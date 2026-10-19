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
use crate::session::Provenance;
use crate::source_provider::document_sources;
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

/// Rules returned by a `content` request that sets no `max` (ADR-0024).
const DEFAULT_CONTENT_RULES: usize = 20;

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

struct Snapshot {
    loaded: Loaded,
    semantic: Arc<SemanticService<'static>>,
    lint_semantic: Option<SemanticService<'static>>,
    /// The disk state `loaded` was read from, recomputed before every
    /// program-reading request so a long-lived session observes edits, file
    /// additions, and removals (#471).
    fingerprint: input::DiskFingerprint,
}

/// The session-aware tool service.
pub struct ToolService<'a> {
    session: &'a mut CompilerSession,
    /// The current program snapshot, when one exists. The service may exist
    /// without one (#512): a configured project that cannot load at
    /// construction is deferred to the first program-reading request rather
    /// than failing the service, so `None` is the unloaded state — never an
    /// empty placeholder program.
    snapshot: Option<Snapshot>,
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
            snapshot: None,
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
        self.snapshot.as_ref().map(|snapshot| &snapshot.loaded)
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
    ///
    /// `check` and `compile` are workflows, not snapshot queries (#591):
    /// they run the session's operation-aware load instead of requiring a
    /// canonical snapshot, so a compile-capable provider's `check` compiles
    /// for canonical validation and supplies the snapshot later queries
    /// read (#617); a provider that declines its compile keeps the
    /// check-only path.
    pub fn handle(&mut self, request: &ToolRequest) -> ToolResponse {
        let response = self.handle_full(request);
        let ToolResponse::Ok { mut result } = response else {
            return response;
        };
        match request {
            ToolRequest::Check { all } => crate::essential::envelope(&mut result, *all),
            ToolRequest::Analyze { all, .. } => {
                result["result"] = crate::essential::analyze(&result["result"], *all);
                crate::essential::envelope(&mut result, *all);
            }
            ToolRequest::Inspect { all } => {
                result["result"] = crate::essential::inspect(&result["result"], *all);
                crate::essential::envelope(&mut result, *all);
            }
            ToolRequest::Lint { all, .. } => result = crate::essential::lint(&result, *all),
            ToolRequest::References { symbol, all, .. } => {
                let id = match self.symbol_address(symbol) {
                    Ok(id) => id,
                    Err(error) => return ToolResponse::Error { error },
                };
                let usage = match self.semantic_query(Request::GetUsage { symbol: id }) {
                    ToolResponse::Ok { result } => result,
                    other => return other,
                };
                let mut payload = usage;
                if let Some(selected) = result.as_object_mut() {
                    payload["references"] = selected.remove("references").unwrap_or_default();
                    if let Some(selection) = selected.remove("selection") {
                        payload["selection"] = selection;
                    }
                } else {
                    payload["references"] = result;
                }
                result = crate::essential::query(&payload, *all);
            }
            ToolRequest::CostEstimate { all, .. } => result = crate::essential::cost(&result, *all),
            ToolRequest::Rules { all, .. }
            | ToolRequest::Symbols { all, .. }
            | ToolRequest::Cfg { all, .. }
            | ToolRequest::Findings { all, .. }
            | ToolRequest::PersistentObjects { all }
            | ToolRequest::CallGraph { all, .. } => result = crate::essential::query(&result, *all),
            _ => {}
        }
        ToolResponse::Ok { result }
    }

    pub(crate) fn handle_full(&mut self, request: &ToolRequest) -> ToolResponse {
        match request {
            ToolRequest::Check { .. } | ToolRequest::Compile { .. } => self.refresh_workflow(),
            _ if Self::reads_program(request) => {
                if let Err(error) = self.refresh() {
                    return ToolResponse::Error { error };
                }
                if let Some(error) = self.stale_id(request) {
                    return ToolResponse::Error { error };
                }
            }
            _ => {}
        }
        let response = hotpath::measure_block!(request_label(request), self.dispatch(request));
        self.note_id_space(request, &response);
        self.adopt_session_snapshot();
        response
    }

    /// Whether the request requires a canonical snapshot to answer (#471,
    /// #591). `check`/`compile` do not: they run their own operation-aware
    /// load, so a provider without canonical compile capability still
    /// answers `check`. A provider operation carrying its own document set
    /// consults no snapshot either; one that omits it derives the set from
    /// the loaded project (#548) and so requires it like every other
    /// program-reading operation.
    fn reads_program(request: &ToolRequest) -> bool {
        !matches!(
            request,
            ToolRequest::Check { .. }
                | ToolRequest::Compile { .. }
                | ToolRequest::Capabilities
                | ToolRequest::TargetMetadata
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
        let Some(snapshot) = &self.snapshot else {
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
        let fingerprint = input::disk_fingerprint(&self.session.config, &snapshot.loaded.input);
        if snapshot.fingerprint == fingerprint {
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
        self.clear_snapshot();
        self.invalidated = true;
    }

    fn clear_snapshot(&mut self) {
        self.snapshot = None;
        self.symbol_ids_current = false;
        self.rule_ids_current = false;
    }

    /// The freshness gate for `check`/`compile` (#591): like [`Self::refresh`]
    /// an observed disk change invalidates the served snapshot, but the
    /// workflow itself performs the operation-aware load — the session never
    /// demands a canonical compile just to run `check`, and the load's
    /// outcome rides in the workflow envelope rather than a `ToolResponse`
    /// refusal. No snapshot held means there is nothing to invalidate: the
    /// workflow resolves the input itself.
    fn refresh_workflow(&mut self) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let fingerprint = input::disk_fingerprint(&self.session.config, &snapshot.loaded.input);
        if snapshot.fingerprint == fingerprint {
            return;
        }
        self.session.drop_loaded();
        self.clear_snapshot();
        self.reloads += 1;
    }

    /// Adopt the canonical snapshot a `check`/`compile` workflow established
    /// when the service holds none (#591): a successful `compile` makes its
    /// program the snapshot later queries serve — the same guarantee as a
    /// session that had already compiled before service construction. A
    /// check-only provider's `check` caches no program (`cached_loaded` is
    /// `None`), so snapshot queries keep refusing `load()`'s explicit
    /// unsupported state rather than inheriting a placeholder program.
    fn adopt_session_snapshot(&mut self) {
        if self.snapshot.is_none() {
            if let Some(loaded) = self.session.cached_loaded() {
                self.adopt(loaded);
            }
        }
    }

    fn loader_error(diagnostic: Diagnostic) -> ToolErrorInfo {
        ToolErrorInfo {
            code: diagnostic.code,
            message: diagnostic.message,
        }
    }

    fn adopt(&mut self, loaded: Loaded) {
        let semantic = self.session.shared_semantic(&loaded);
        let lint_semantic = (!self.session.config.lint.rules.is_empty())
            .then(|| semantic.with_lint_config(self.session.config.lint.clone()));
        let fingerprint = input::disk_fingerprint(&self.session.config, &loaded.input);
        self.snapshot = Some(Snapshot {
            loaded,
            semantic,
            lint_semantic,
            fingerprint,
        });
    }

    fn snapshot(&self) -> &Snapshot {
        self.snapshot
            .as_ref()
            .expect("the refresh gate guarantees a snapshot")
    }

    fn snapshot_and_semantic(&self) -> (Loaded, Arc<SemanticService<'static>>) {
        let snapshot = self.snapshot();
        (snapshot.loaded.clone(), Arc::clone(&snapshot.semantic))
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
        let loaded = &self.snapshot().loaded;
        // Mapped members are provider-file-table spellings relative to the
        // project root the provider was given; unmapped members are the
        // input's own display spellings, relative to the invocation cwd.
        let base = if loaded.provenance == Provenance::Mapped {
            &loaded.input.root
        } else {
            &loaded.input.cwd
        };
        crate::source_provider::provider_document_set(&loaded.source_files, base, language_id)
    }

    fn semantic(&self) -> &SemanticService<'static> {
        &self.snapshot().semantic
    }

    /// Refuse a numeric id that was issued by an earlier program (#471):
    /// `stale-id` until the client observes the current space. Every
    /// numeric address a request carries is checked — a `references` rule
    /// filter is the same rule-space address `cfg` consumes (#592). Name
    /// addressing resolves against the loaded index and is always fresh.
    fn stale_id(&self, request: &ToolRequest) -> Option<ToolErrorInfo> {
        let stale = match request {
            ToolRequest::References { symbol, rule, .. } => {
                (matches!(symbol, Address::Id(_)) && !self.symbol_ids_current)
                    || (matches!(rule, Some(Address::Id(_))) && !self.rule_ids_current)
            }
            ToolRequest::Usage { symbol } => {
                matches!(symbol, Address::Id(_)) && !self.symbol_ids_current
            }
            ToolRequest::Cfg { rule, .. } => {
                matches!(rule, Address::Id(_)) && !self.rule_ids_current
            }
            ToolRequest::Rules {
                rule: Some(Address::Id(_)),
                ..
            } => !self.rule_ids_current,
            _ => false,
        };
        stale.then(|| ToolErrorInfo {
            code: "stale-id".to_string(),
            message: "the loaded project changed on disk; numeric symbol ids and rule indexes issued before the reload are no longer valid — list `symbols` or `rules` again, or address by name".to_string(),
        })
    }

    /// A request that names the current id space re-establishes it (#471): a
    /// successful `symbols`/`rules` listing, or an `ambiguous-*` refusal that
    /// already reports the current candidates.
    fn note_id_space(&mut self, request: &ToolRequest, response: &ToolResponse) {
        match response {
            ToolResponse::Ok { .. } => match request {
                ToolRequest::Symbols { .. } => self.symbol_ids_current = true,
                ToolRequest::Rules { .. } => self.rule_ids_current = true,
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
            ToolRequest::Compile { output, force, to } => {
                let envelope = self.session.compile_to(crate::CompileWrite {
                    dest: output
                        .clone()
                        .map_or(crate::OutputDest::Default, crate::OutputDest::Path),
                    overwrite: if *force {
                        crate::Overwrite::Force
                    } else {
                        crate::Overwrite::Refuse
                    },
                    to: to.clone(),
                });
                let result = serde_json::to_value(envelope).expect("compile result serializes");
                ToolResponse::Ok { result }
            }
            ToolRequest::Check { .. } => {
                let result =
                    serde_json::to_value(self.session.check()).expect("check result serializes");
                ToolResponse::Ok { result }
            }
            ToolRequest::Analyze { selection, all: _ } => {
                if let Some(error) = self.selection_error(selection) {
                    return error;
                }
                let (loaded, semantic) = self.snapshot_and_semantic();
                let result =
                    serde_json::to_value(self.session.analyze_loaded(loaded, &semantic, selection))
                        .expect("analyze result serializes");
                ToolResponse::Ok { result }
            }
            ToolRequest::Inspect { all: _ } => {
                let (loaded, semantic) = self.snapshot_and_semantic();
                let result = serde_json::to_value(self.session.inspect_loaded(loaded, &semantic))
                    .expect("inspect result serializes");
                ToolResponse::Ok { result }
            }
            ToolRequest::Project => self.ok(self.project()),
            ToolRequest::Rules {
                name,
                rule,
                file,
                max,
                content,
                all,
            } => self.rules(
                name,
                rule.as_ref(),
                file,
                &max.or_else(|| (*all && *content).then_some(usize::MAX)),
                *content,
            ),
            ToolRequest::Symbols {
                kind, file, max, ..
            } => self.symbols(kind.as_deref(), file.as_deref(), *max),
            ToolRequest::References {
                symbol,
                kind,
                rule,
                file,
                max,
                ..
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
            ToolRequest::Cfg {
                rule, kind, max, ..
            } => match self.rule_address(rule) {
                Ok(rule) => self.cfg(rule, kind.as_deref(), *max),
                Err(error) => ToolResponse::Error { error },
            },
            ToolRequest::Findings { selection, .. } => self.findings(selection),
            ToolRequest::PersistentObjects { .. } => self.persistent_objects(),
            ToolRequest::Lint { selection, all: _ } => self.lint(selection),
            ToolRequest::LintRules => self.configured_semantic().handle(&Request::LintRules),
            ToolRequest::CallGraph {
                caller,
                callee,
                max,
                ..
            } => self.call_graph(caller.as_deref(), callee.as_deref(), *max),
            ToolRequest::CostEstimate {
                selection, rule, ..
            } => self.cost_estimate(selection, rule.as_deref()),
            ToolRequest::TargetMetadata => self.ok(self.target_metadata()),
            ToolRequest::ValidateEdit {
                sources,
                transaction,
            } => self.ok(serde_json::to_value(
                self.session
                    .validate_edit_transaction(sources.as_ref(), transaction),
            )
            .expect("serializes")),
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
    /// A changed input drops the stale snapshot so `compile`'s own load
    /// re-resolves it (#471); a successful load supplies the snapshot later
    /// queries read (#591). A failed load surfaces as this envelope's
    /// diagnostics — the stale snapshot is never consulted (#512).
    pub fn compile(&mut self) -> Envelope<CompileResult> {
        self.refresh_workflow();
        let envelope = self.session.compile();
        self.adopt_session_snapshot();
        envelope
    }

    /// Check through the shared session pipeline.
    ///
    /// The same freshness contract as [`Self::compile`] applies (#471),
    /// minus the canonical-snapshot requirement: `check` runs the
    /// operation-aware load, which on a compile-capable provider compiles
    /// for canonical validation (#617) and on a provider that declines its
    /// compile answers the check operation alone (#591).
    pub fn check(&mut self) -> Envelope<CheckResult> {
        self.refresh_workflow();
        let envelope = self.session.check();
        self.adopt_session_snapshot();
        envelope
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
        let (loaded, semantic) = self.snapshot_and_semantic();
        let selection = self.session.config.selection.clone();
        self.session.analyze_loaded(loaded, &semantic, &selection)
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
        let (loaded, semantic) = self.snapshot_and_semantic();
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
        match self.semantic_query(Request::GetFindings) {
            ToolResponse::Ok { result } => {
                let findings = crate::session::prepare_findings(
                    result,
                    self.semantic(),
                    &self.snapshot().loaded,
                    self.session.catalog(),
                );
                let (findings, outcome) = selection.apply_findings(
                    findings,
                    &crate::select::file_bases(&self.snapshot().loaded.input),
                );
                match outcome {
                    Some(outcome) => self.ok(json!({ "findings": findings, "selection": outcome })),
                    None => self.ok(serde_json::Value::Array(findings)),
                }
            }
            other => other,
        }
    }

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
                crate::session::resolve_span_paths(&mut result, &self.snapshot().loaded);
                ToolResponse::Ok { result }
            }
            other => other,
        }
    }

    fn semantic_query(&self, request: Request) -> ToolResponse {
        self.semantic().handle(&request)
    }

    fn configured_semantic(&self) -> &SemanticService<'static> {
        let snapshot = self.snapshot();
        snapshot
            .lint_semantic
            .as_ref()
            .unwrap_or(&snapshot.semantic)
    }

    /// A selection naming an unknown rule id is a usage error, never a
    /// silent empty result (#430).
    fn selection_error(&self, selection: &crate::select::FindingSelection) -> Option<ToolResponse> {
        self.selection_check(selection)
            .err()
            .map(|error| ToolResponse::Error { error })
    }

    /// The selection's validity as a structured error: an unknown `rule` id
    /// would silently select nothing and report a clean result (#430).
    fn selection_check(
        &self,
        selection: &crate::select::FindingSelection,
    ) -> Result<(), ToolErrorInfo> {
        selection
            .validate(self.session.lint_registry())
            .map_err(|message| ToolErrorInfo {
                code: "invalid-selection".to_string(),
                message,
            })
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
            &crate::select::file_bases(&self.snapshot().loaded.input),
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
    /// fields are `name` (a declared rule name), `rule` (one rule address),
    /// `file`, and `max` (#531). With `content`, each selected rule carries
    /// its canonical content, `max` defaults to [`DEFAULT_CONTENT_RULES`],
    /// and the selection shape is always returned (ADR-0024).
    fn rules(
        &self,
        name: &Option<String>,
        rule: Option<&Address>,
        file: &Option<String>,
        max: &Option<usize>,
        content: bool,
    ) -> ToolResponse {
        if let Some(name) = name {
            if !self
                .snapshot()
                .loaded
                .program
                .rules
                .iter()
                .any(|rule| rule.name == *name)
            {
                return invalid_selection(format!("unknown rule '{name}'"));
            }
        }
        let rule_id = match rule {
            Some(address) => match self.rule_address(address) {
                Ok(id) if (id as usize) < self.snapshot().loaded.program.rules.len() => {
                    Some(id as u64)
                }
                Ok(id) => return invalid_selection(format!("unknown rule {id}")),
                Err(error) if error.code == "unknown-rule" => {
                    return invalid_selection(error.message);
                }
                Err(error) => return ToolResponse::Error { error },
            },
            None => None,
        };
        let max = if content {
            max.or(Some(DEFAULT_CONTENT_RULES))
        } else {
            *max
        };
        let selected =
            content || name.is_some() || rule_id.is_some() || file.is_some() || max.is_some();
        let response = self.semantic_query_with_resolved_span_paths(Request::ListRules);
        let response = match response {
            ToolResponse::Ok { result } => self.select_result(
                "rules",
                result,
                file.as_deref(),
                |item| {
                    name.as_deref()
                        .is_none_or(|name| item["name"].as_str() == Some(name))
                        && rule_id.is_none_or(|id| item["id"].as_u64() == Some(id))
                },
                max,
                selected,
            ),
            other => other,
        };
        if content {
            self.attach_rule_content(response)
        } else {
            response
        }
    }

    fn attach_rule_content(&self, response: ToolResponse) -> ToolResponse {
        let ToolResponse::Ok { mut result } = response else {
            return response;
        };
        // OverPy spellings for the canonical ids need a provider capability
        // that no provider advertises yet (ADR-0024 decision 4).
        if self.snapshot().loaded.input.kind == crate::config::SourceKind::Opy {
            result["unavailable"] = json!({"names": "opy-rs"});
        }
        let Some(rules) = result["rules"].as_array_mut() else {
            return ToolResponse::Ok { result };
        };
        for rule in rules {
            let id = rule["id"].as_u64().unwrap_or_default() as u32;
            match self.semantic_query_with_resolved_span_paths(Request::GetRuleContent { rule: id })
            {
                ToolResponse::Ok { result: content } => rule["content"] = content,
                other => return other,
            }
        }
        ToolResponse::Ok { result }
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
                Ok(id) if (id as usize) < self.snapshot().loaded.program.rules.len() => Some(id),
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
        let program = &self.snapshot().loaded.program;
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
            .filter(|rule| !rule.disabled)
            .flat_map(|rule| {
                wright_analyzer::canonical::live_actions(&rule.actions).filter_map(
                    move |(_, action)| {
                        wright_analyzer::canonical::subroutine_callee(action)
                            .map(|callee| json!({ "caller": rule.name, "callee": callee }))
                    },
                )
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
    /// (#430). Wire presentation omits configuration and caps findings.
    fn lint(&self, selection: &crate::select::FindingSelection) -> ToolResponse {
        if let Some(error) = self.selection_error(selection) {
            return error;
        }
        let service = self.configured_semantic();
        let lint_rules = match service.handle(&Request::LintRules) {
            Response::Ok { result } => result,
            Response::Error { .. } => serde_json::json!({}),
        };
        let findings = match service.handle(&Request::GetFindings) {
            Response::Ok { result } => result,
            Response::Error { .. } => serde_json::json!([]),
        };
        let findings = crate::session::prepare_findings(
            findings,
            service,
            &self.snapshot().loaded,
            self.session.catalog(),
        );
        let ok = !findings
            .iter()
            .any(|finding| finding["severity"] == "error");
        let (findings, outcome) = selection.apply_findings(
            findings,
            &crate::select::file_bases(&self.snapshot().loaded.input),
        );
        let (rules, config, skipped) = crate::result::split_lint_rules(&lint_rules);
        let mut result = json!({
            "inputIdentity": self.snapshot().loaded.input.identity,
            "ok": ok,
            "exit": if ok { 0 } else { crate::result::exit::SOURCE_ERROR },
            "rules": rules,
            "config": config,
            "findings": findings,
            "skipped": skipped,
        });
        if let Some(outcome) = outcome {
            result["selection"] = serde_json::to_value(outcome).expect("selection serializes");
        }
        self.ok(result)
    }

    fn project(&self) -> serde_json::Value {
        let loaded = &self.snapshot().loaded;
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
    /// Findings and skipped rule evaluations come from the same configured
    /// semantic service `lint` serves (#788); the findings subset honors
    /// the shared selection (#430) and findings carry resolved span paths
    /// like `lint` (#722), so a `file` selection can match them. The `rule`
    /// selector (#724) reports one declared rule's element-cost breakdown
    /// instead of the whole-program view.
    fn cost_estimate(
        &self,
        selection: &crate::select::FindingSelection,
        rule: Option<&str>,
    ) -> ToolResponse {
        match self.cost_estimate_result(selection, rule) {
            Ok((result, _)) => self.ok(result),
            Err(error) => ToolResponse::Error { error },
        }
    }

    /// The `costEstimate` payload plus the applied selection's outcome: the
    /// result body is the wire payload; the outcome travels in-process so
    /// the session's verdict can reflect the full finding set (#635).
    /// `rule` selects the per-rule drill-down (#724); `None` keeps the
    /// whole-program view.
    pub(crate) fn cost_estimate_result(
        &self,
        selection: &crate::select::FindingSelection,
        rule: Option<&str>,
    ) -> Result<(serde_json::Value, Option<crate::select::SelectionOutcome>), ToolErrorInfo> {
        self.selection_check(selection)?;
        let loaded = &self.snapshot().loaded;
        if let Some(rule) = rule {
            if selection.is_active() {
                return Err(ToolErrorInfo {
                    code: "invalid-selection".to_string(),
                    message: "the 'rule' selector reports one rule's element cost; finding selection fields do not apply"
                        .to_string(),
                });
            }
            return self
                .cost_estimate_rule(loaded, rule)
                .map(|result| (result, None));
        }
        let locale = CompilerSession::locale_for(loaded);
        let emitted = workshop_rs::emitter::emit(&loaded.program, self.session.catalog(), &locale);
        let live = loaded
            .program
            .rules
            .iter()
            .filter(|rule| !rule.disabled)
            .flat_map(|rule| wright_analyzer::canonical::live_actions(&rule.actions));
        let mut program_actions = 0;
        let mut waits = 0;
        for (_, action) in live {
            program_actions += 1;
            if matches!(action, workshop_rs::Action::Call { name, .. } if name == "wait") {
                waits += 1;
            }
        }
        // The same finding set `lint`/`analyze` serve for this input, kind,
        // locale and lint configuration — the configured semantic service's
        // registry run, resolved span paths included (#722, #788) — never a
        // second analyzer pass that could disagree with them.
        let service = self.configured_semantic();
        let findings = match service.handle(&Request::GetFindings) {
            Response::Ok { result } => result,
            Response::Error { .. } => serde_json::json!([]),
        };
        let findings =
            crate::session::prepare_findings(findings, service, loaded, self.session.catalog());
        // Rule evaluations skipped because a rule's canonical CFG is
        // unavailable are disclosed exactly like `lint` reports them —
        // findings the registry could not run stay accounted for (#788).
        let lint_rules = match service.handle(&Request::LintRules) {
            Response::Ok { result } => result,
            Response::Error { .. } => serde_json::json!({}),
        };
        let (_, _, skipped) = crate::result::split_lint_rules(&lint_rules);
        let (findings, outcome) =
            selection.apply_findings(findings, &crate::select::file_bases(&loaded.input));
        // Unknown is not a value (ADR-0030 decision 3): a computation that
        // failed reports `null` plus an `<x>Unavailable` reason instead of a
        // fabricated zero, and a known limit such as the element budget is
        // still reported.
        let mut exact = json!({
            "programActions": program_actions,
            // Every declared rule, disabled included — the same program
            // inventory `analyze`/`inspect` report and the same set
            // `elementCount.rules` enumerates (#788).
            "programRules": loaded.program.rules.len(),
            "waitActions": waits,
        });
        match &emitted {
            Ok(text) => exact["emittedBytes"] = json!(text.len()),
            Err(error) => {
                exact["emittedBytes"] = serde_json::Value::Null;
                exact["emittedBytesUnavailable"] = json!(error.to_string());
            }
        }
        let mut result = json!({
            "exact": exact,
            "findings": findings,
            "skipped": skipped,
            "kind": {
                "exact": "exact target-resource counts",
                "elementCount": "canonical Workshop structure counted against the client element budget; not a statement of client importability",
                "findings": "static/heuristic execution indicators",
                "performance": "compiler-host performance is measured by wright-bench, not in-process",
            },
        });
        // `report.rules` keeps `program.rules` canonical order, disabled
        // rules included, so `index` names the rule (#691). Payload order is
        // ranked by count, largest first — the generic item cap then keeps
        // the largest rules and `elementCount.truncation.rules` describes the
        // cut.
        match loaded.program.element_count(self.session.catalog()) {
            Ok(report) => {
                let mut rules: Vec<_> = report.rules.iter().enumerate().collect();
                rules.sort_by_key(|(index, node)| (std::cmp::Reverse(node.count), *index));
                let rule_entries: Vec<_> = rules
                    .iter()
                    .map(|(index, node)| {
                        // The entry's span is the same span `inspect rules`
                        // reports for the rule — resolved `span.path` from
                        // `program.rule_span`, not the report node's own
                        // span, so both surfaces agree (#722).
                        let mut entry = json!({
                            "index": index,
                            "name": loaded.program.rules[*index].name,
                            "count": node.count,
                            "span": wright_analyzer::canonical::span_json(
                                loaded.program.rule_span(*index),
                            ),
                        });
                        crate::session::resolve_span_paths(&mut entry, loaded);
                        entry
                    })
                    .collect();
                result["elementCount"] = json!({
                    "total": report.total,
                    "budget": crate::session::WORKSHOP_CLIENT_ELEMENT_LIMIT,
                    "rules": rule_entries,
                });
            }
            Err(error) => {
                // The count itself is absent, not zero; the known client
                // budget still reports (ADR-0030 decision 3).
                result["elementCount"] = json!({
                    "budget": crate::session::WORKSHOP_CLIENT_ELEMENT_LIMIT,
                });
                result["elementCountUnavailable"] = json!(error.to_string());
            }
        }
        if let Some(outcome) = &outcome {
            result["selection"] = serde_json::to_value(outcome).expect("selection serializes");
        }
        Ok((result, outcome))
    }

    /// The CLI-side run-cache comparison and `--max-total` gate for
    /// `inspect cost` (ADR-0027, #723): `--max-total` fails with the
    /// overage named and reads no cache; an explicitly named baseline that
    /// does not exist is a usage error outside CI — under CI it degrades
    /// to no comparison silently (ADR-0030 decision 14); an
    /// absent/corrupt/unknown-version record just omits the comparison.
    /// Agent `costEstimate` requests never reach here — the agent surface
    /// takes its reference as inline data and returns no implicit
    /// comparison (decision 3).
    ///
    /// Returns the comparison for the caller to carry in-process; it is
    /// never serialized into the JSON or agent result.
    pub(crate) fn apply_cost_run(
        &self,
        result: &serde_json::Value,
    ) -> Result<Option<crate::cache::Comparison>, ToolErrorInfo> {
        let loaded = &self.snapshot().loaded;
        let run = &self.session.config.run;
        // `--max-total`: exit 1 below the current total, naming the
        // overage; identical with and without a cache (decision 2).
        if let (Some(max), Some(total)) = (run.max_total, result["elementCount"]["total"].as_u64())
        {
            if total as usize > max {
                return Err(ToolErrorInfo {
                    code: "cost-over-budget".to_string(),
                    message: format!(
                        "element total {total} exceeds --max-total {max} by {}",
                        total as usize - max
                    ),
                });
            }
        }
        let cost_run = self.session.cost_run(loaded);
        if run.baseline.is_some()
            && !crate::cache::ci()
            && !crate::cache::baseline_exists(&cost_run)
        {
            return Err(ToolErrorInfo {
                code: "invalid-selection".to_string(),
                message: format!(
                    "unknown baseline '{}': no record under .wright/ for this input kind/locale/profile",
                    run.baseline.as_deref().unwrap_or_default()
                ),
            });
        }
        let Some(snapshot) = self.session.cost_snapshot(loaded) else {
            return Ok(None);
        };
        Ok(crate::cache::compare_and_record(&cost_run, &snapshot))
    }

    /// The `costEstimate` drill-down for one declared rule name (#724):
    /// the rule's identity and location, its direct statements ranked by
    /// recursive `count`, and the per-name sum of each node's own
    /// (`base_count + adjustment`) contribution so the shares reconcile to
    /// the rule total without double-counting nested nodes.
    fn cost_estimate_rule(
        &self,
        loaded: &Loaded,
        rule: &str,
    ) -> Result<serde_json::Value, ToolErrorInfo> {
        let rule_id = self
            .semantic()
            .resolve_rule(rule)
            .map_err(|error| ToolErrorInfo {
                code: "invalid-selection".to_string(),
                message: error.message,
            })?;
        let report = loaded
            .program
            .element_count(self.session.catalog())
            .map_err(|error| ToolErrorInfo {
                code: "element-count-unavailable".to_string(),
                message: error.to_string(),
            })?;
        let node = report
            .rules
            .get(rule_id)
            .expect("report rules keep program.rules order");
        let mut statements: Vec<_> = node
            .children
            .iter()
            .map(|child| {
                let mut statement = json!({
                    "name": child.name,
                    "count": child.count,
                    "span": wright_analyzer::canonical::span_json(child.span),
                });
                crate::session::resolve_span_paths(&mut statement, loaded);
                statement
            })
            .collect();
        statements.sort_by_key(|statement| std::cmp::Reverse(statement["count"].as_u64()));

        // `byNode`: each node's own cost (base + adjustment, children
        // excluded) summed per canonical node name, so entries over all
        // names add up to the rule's `count`. Occurrences count nodes, not
        // elements.
        let mut by_name: std::collections::BTreeMap<&str, (usize, usize)> =
            std::collections::BTreeMap::new();
        fn own_cost<'a>(
            node: &'a workshop_rs::actions::ElementCountNode,
            by_name: &mut std::collections::BTreeMap<&'a str, (usize, usize)>,
        ) {
            let own = (node.base_count as isize + node.adjustment).max(0) as usize;
            let entry = by_name.entry(node.name.as_str()).or_default();
            entry.0 += own;
            entry.1 += 1;
            for child in &node.children {
                own_cost(child, by_name);
            }
        }
        own_cost(node, &mut by_name);
        let mut by_node: Vec<_> = by_name
            .iter()
            .map(|(name, (count, occurrences))| {
                json!({"name": name, "count": count, "occurrences": occurrences})
            })
            .collect();
        by_node.sort_by_key(|entry| std::cmp::Reverse(entry["count"].as_u64()));

        let mut result = json!({
            "rule": {
                "id": rule_id,
                "name": loaded.program.rules[rule_id].name,
                "count": node.count,
                "span": wright_analyzer::canonical::span_json(
                    loaded.program.rule_span(rule_id),
                ),
            },
            "statements": statements,
            "byNode": by_node,
        });
        crate::session::resolve_span_paths(&mut result["rule"], loaded);
        Ok(result)
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

pub(crate) fn json_span_path(item: &serde_json::Value) -> Option<&str> {
    item.get("span")
        .and_then(|span| span.get("path"))
        .and_then(serde_json::Value::as_str)
}

#[cfg_attr(not(feature = "hotpath"), allow(dead_code))]
fn request_label(request: &ToolRequest) -> &'static str {
    match request {
        ToolRequest::Capabilities => "req::capabilities",
        ToolRequest::Compile { .. } => "req::compile",
        ToolRequest::Check { .. } => "req::check",
        ToolRequest::Analyze { .. } => "req::analyze",
        ToolRequest::Inspect { .. } => "req::inspect",
        ToolRequest::Project => "req::project",
        ToolRequest::Rules { .. } => "req::rules",
        ToolRequest::Symbols { .. } => "req::symbols",
        ToolRequest::References { .. } => "req::references",
        ToolRequest::Usage { .. } => "req::usage",
        ToolRequest::Cfg { .. } => "req::cfg",
        ToolRequest::Findings { .. } => "req::findings",
        ToolRequest::PersistentObjects { .. } => "req::persistentObjects",
        ToolRequest::Lint { .. } => "req::lint",
        ToolRequest::LintRules => "req::lintRules",
        ToolRequest::CallGraph { .. } => "req::callGraph",
        ToolRequest::CostEstimate { .. } => "req::costEstimate",
        ToolRequest::TargetMetadata => "req::targetMetadata",
        ToolRequest::ValidateEdit { .. } => "req::validateEdit",
        ToolRequest::ProviderValidateEdit { .. } => "req::providerValidateEdit",
        ToolRequest::Lookup { .. } => "req::lookup",
    }
}
