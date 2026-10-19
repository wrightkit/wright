// Generated from schemas/wright-agent-v1.schema.json by ui/scripts/generate-types.mjs — do not edit.
/* eslint-disable */

export type WrightAgentContractV1 = ToolRequest | ToolResponse;
export type ToolRequest =
  | CapabilitiesRequest
  | CompileRequest
  | CheckRequest
  | AnalyzeRequest
  | InspectRequest
  | ProjectRequest
  | RulesRequest
  | SymbolsRequest
  | ReferencesRequest
  | UsageRequest
  | CfgRequest
  | FindingsRequest
  | PersistentObjectsRequest
  | LintRequest
  | LintRulesRequest
  | CallGraphRequest
  | CostEstimateRequest
  | TargetMetadataRequest
  | ValidateEditTransactionRequest
  | ProviderValidateEditRequest
  | LookupRequest;
export type ToolResponse =
  | {
      result: AgentResult;
      [k: string]: unknown | undefined;
    }
  | {
      error: ToolErrorInfo;
      [k: string]: unknown | undefined;
    };
export type AgentResult =
  | Capabilities
  | CompileResult
  | CheckResult
  | AnalyzeResult
  | InspectResult
  | ProjectResult
  | RulesResult
  | SymbolsResult
  | ReferencesResult
  | UsageResult
  | CfgResult
  | FindingsResult
  | PersistentObjectsResult
  | LintPayload
  | LintRulesResult
  | CallGraphResult
  | CostEstimateResult
  | TargetMetadataResult
  | EditValidationPayload
  | ProviderMutationPayload
  | LookupResult;
export type CompileResult = WorkflowEnvelope & {
  command?: "compile";
  result?: CompilePayload;
  [k: string]: unknown | undefined;
};
export type CheckResult = WorkflowEnvelope & {
  command?: "check";
  result?: CheckPayload;
  [k: string]: unknown | undefined;
};
export type AnalyzeResult = WorkflowEnvelope & {
  command?: "analyze";
  result?: AnalyzePayload;
  [k: string]: unknown | undefined;
};
export type InspectResult = WorkflowEnvelope & {
  command?: "inspect";
  result?: InspectPayload;
  [k: string]: unknown | undefined;
};
export type RulesResult = RuleSummary[] | RulesSelectionResult | CappedListResult;
export type Span = null | SourceSpan;
export type SymbolsResult = Symbol[] | SymbolsSelectionResult | CappedListResult;
export type ReferencesResult = UsageResult & {
  references: ReferencesListResult;
  [k: string]: unknown | undefined;
};
export type ReferencesListResult = Reference[] | ReferencesSelectionResult | CappedListResult;
export type FindingsResult = Finding[] | FindingsSelectionResult | CappedListResult;
export type PersistentObjectsResult =
  | {
      kind: string;
      rule: number;
      action: number;
      executionScope: string;
      visibility: string;
      reevaluation: null | {
        domain: string;
        mode: string;
        [k: string]: unknown | undefined;
      };
      identityRetained: boolean;
      sameKindCleanupInRule: boolean;
      span: Span;
      [k: string]: unknown | undefined;
    }[]
  | CappedListResult;
export type CallGraphResult = CallGraphEdge[] | CallGraphSelectionResult | CappedListResult;

export interface CapabilitiesRequest {
  op: "capabilities";
}
export interface CompileRequest {
  op: "compile";
  /**
   * File to write the program to; default `<entry stem>.ow` in the service's working directory.
   */
  output?: string;
  /**
   * Replace an existing output file. The service never prompts, so a repeat compile to the same file needs force: true. The input source file is never overwritten.
   */
  force?: boolean;
  /**
   * Output locale for the emitted Workshop text (e.g. "en-US", "zh-CN"); defaults to the loaded input's locale. Use it to convert Workshop text between client locales independently of the input locale.
   */
  to?: string;
}
export interface CheckRequest {
  op: "check";
  /**
   * Return every list item instead of the default cap; explicit filters and max still apply.
   */
  all?: boolean;
}
export interface AnalyzeRequest {
  op: "analyze";
  severity?: "error" | "warning" | "info";
  rule?: string;
  file?: string;
  max?: number;
  /**
   * Return every list item instead of the default cap; explicit filters and max still apply.
   */
  all?: boolean;
}
export interface InspectRequest {
  op: "inspect";
  /**
   * Return every list item instead of the default cap; explicit filters and max still apply.
   */
  all?: boolean;
}
export interface ProjectRequest {
  op: "project";
}
export interface RulesRequest {
  op: "rules";
  /**
   * Selection (#531): only the rule with this declared name.
   */
  name?: string;
  /**
   * Selection (ADR-0024): exactly this rule, addressed by id or declared name.
   */
  rule?: number | string;
  /**
   * Selection (#531): only rules located in this source file; any spelling that resolves to the same file selects it.
   */
  file?: string;
  /**
   * Selection (#531): report at most this many rules; the response's `selection.withheld` counts what the bound dropped.
   */
  max?: number;
  /**
   * ADR-0024: add each returned rule's canonical content (`workshop-rs/rule-content-v1`). Without `max`, at most 20 rules are returned; the selection-wrapped result is always used.
   */
  content?: boolean;
  /**
   * Return every list item instead of the default cap; explicit filters and max still apply.
   */
  all?: boolean;
}
export interface SymbolsRequest {
  op: "symbols";
  /**
   * Selection (#531): only symbols of this kind.
   */
  kind?: "globalVariable" | "playerVariable" | "subroutine" | "rule" | null;
  /**
   * Selection (#531): only symbols located in this source file; any spelling that resolves to the same file selects it.
   */
  file?: string;
  /**
   * Selection (#531): report at most this many symbols; the response's `selection.withheld` counts what the bound dropped.
   */
  max?: number;
  /**
   * Return every list item instead of the default cap; explicit filters and max still apply.
   */
  all?: boolean;
}
export interface ReferencesRequest {
  op: "references";
  /**
   * Symbol id or declared name. A numeric id captured before the project changed on disk is refused `stale-id` until a `symbols` listing re-establishes the space; names are always current.
   */
  symbol: number | string;
  /**
   * Selection (#531): only references of this kind.
   */
  kind?: "declaration" | "definition" | "read" | "write" | "call";
  /**
   * Selection (#531): only references inside this rule, addressed by index or declared name.
   */
  rule?: number | string;
  /**
   * Selection (#531): only references located in this source file; any spelling that resolves to the same file selects it.
   */
  file?: string;
  /**
   * Selection (#531): report at most this many references; the response's `selection.withheld` counts what the bound dropped.
   */
  max?: number;
  /**
   * Return every list item instead of the default cap; explicit filters and max still apply.
   */
  all?: boolean;
}
export interface UsageRequest {
  op: "usage";
  /**
   * Symbol id or declared name. A numeric id captured before the project changed on disk is refused `stale-id` until a `symbols` listing re-establishes the space; names are always current.
   */
  symbol: number | string;
}
export interface CfgRequest {
  op: "cfg";
  /**
   * Rule index or declared name. A numeric index captured before the project changed on disk is refused `stale-id` until a `rules` listing re-establishes the space; names are always current.
   */
  rule: number | string;
  /**
   * Selection (#531): only blocks of this kind.
   */
  kind?: "entry" | "exit" | "block" | "if" | "while" | "for";
  /**
   * Selection (#531): report at most this many blocks; the response's `selection.withheld` counts what the bound dropped.
   */
  max?: number;
  /**
   * Return every list item instead of the default cap; explicit filters and max still apply.
   */
  all?: boolean;
}
export interface FindingsRequest {
  op: "findings";
  severity?: "error" | "warning" | "info";
  rule?: string;
  file?: string;
  max?: number;
  /**
   * Return every list item instead of the default cap; explicit filters and max still apply.
   */
  all?: boolean;
}
export interface PersistentObjectsRequest {
  op: "persistentObjects";
  /**
   * Return every list item instead of the default cap; explicit filters and max still apply.
   */
  all?: boolean;
}
export interface LintRequest {
  op: "lint";
  severity?: "error" | "warning" | "info";
  rule?: string;
  file?: string;
  max?: number;
  /**
   * Return every list item instead of the default cap; explicit filters and max still apply.
   */
  all?: boolean;
}
export interface LintRulesRequest {
  op: "lintRules";
}
export interface CallGraphRequest {
  op: "callGraph";
  /**
   * Selection (#531): only edges out of this declared rule name.
   */
  caller?: string;
  /**
   * Selection (#531): only edges into this declared subroutine name.
   */
  callee?: string;
  /**
   * Selection (#531): report at most this many edges; the response's `selection.withheld` counts what the bound dropped.
   */
  max?: number;
  /**
   * Return every list item instead of the default cap; explicit filters and max still apply.
   */
  all?: boolean;
}
export interface CostEstimateRequest {
  op: "costEstimate";
  severity?: "error" | "warning" | "info";
  /**
   * A declared rule name selects that rule's element-cost drill-down (#724): `rule` (identity, count, span), `statements[]` (direct children ranked by recursive count) and `byNode[]` (each node's own contribution summed per canonical node name, with occurrences) replace the whole-program `elementCount`/`findings` view. The drill-down reports no findings, so `severity`/`file`/`max` do not apply to it.
   */
  rule?: string;
  file?: string;
  max?: number;
  /**
   * Return every list item instead of the default cap; explicit filters and max still apply.
   */
  all?: boolean;
}
export interface TargetMetadataRequest {
  op: "targetMetadata";
}
export interface ValidateEditTransactionRequest {
  op: "validateEditTransaction";
  /**
   * Optional current texts for the edited sources; omitted or null reads the named files from disk (#472).
   */
  sources?: Sources | null;
  transaction: EditTransaction;
}
/**
 * Source name → full current text. On `validateEditTransaction` the key is the loaded input's source spelling and the field may be omitted (the service reads the named files from disk, #472). On `providerValidateEdit` the keys are `documents` URIs covering every `edits[].source` — an edit into an unsupplied source refuses `edit-unknown-source`. The text is the precondition each edit's `source_identity` verifies against and the text edits apply to.
 */
export interface Sources {
  [k: string]: string | undefined;
}
export interface EditTransaction {
  /**
   * The transaction's edits, applied atomically; edits in one source must not overlap, and two zero-width edits at the same position refuse `edit-zero-width-conflict`.
   *
   * @minItems 1
   */
  edits: [SourceEdit, ...SourceEdit[]];
}
export interface SourceEdit {
  /**
   * The edit kind: `edit` for a caller-proposed replacement; other kinds may appear on service-computed results.
   */
  kind: string;
  /**
   * The source this edit applies to: the loaded input's spelling on `validateEditTransaction`, or a `documents` URI key on the provider operations.
   */
  source: string;
  /**
   * The SHA-256 hex digest of the source's current text — the same digest `project.inputIdentity` reports for the entry source; a text that hashes differently refuses `edit-stale-source`.
   */
  source_identity: string;
  range: EditRange;
  /**
   * The replacement text; empty deletes the range.
   */
  new_text: string;
}
/**
 * A half-open source range in 1-based lines and character columns; `end` is exclusive and a column may point one past the line's last character. A zero-width range inserts `new_text`.
 */
export interface EditRange {
  start_line: number;
  start_col: number;
  end_line: number;
  end_col: number;
}
export interface ProviderValidateEditRequest {
  op: "providerValidateEdit";
  /**
   * The configured source provider's language id — `opy` for the shipped provider.
   */
  language_id: string;
  /**
   * Optional: the caller-owned document set — every `edits[].source` must be a key, and errors in any supplied document block the post-edit check. Omitted or null: the session's loaded project supplies the set — every member read from disk at version 0, keyed by `file://` URI.
   */
  documents?: DocumentSet | null;
  transaction: EditTransaction;
  /**
   * Optional URI → current text covering every `edits[].source` (omitted or null defaults to each document's text); each edit's `source_identity` verifies against it.
   */
  sources?: Sources | null;
  /**
   * Optional project root the provider resolves the project from.
   */
  project_root?: string | null;
}
/**
 * URI → `Document` map: the caller-owned project document set the provider operates on. `providerValidateEdit` refuses an `edits[].source` outside the set with `edit-unknown-source`.
 */
export interface DocumentSet {
  [k: string]: Document | undefined;
}
export interface Document {
  /**
   * The document's URI — also its key in `documents`; provider edit results address documents by it.
   */
  uri: string;
  /**
   * The document's language id (`opy`).
   */
  languageId: string;
  /**
   * The document's current version; provider-computed edits validated against another version refuse `edit-stale-source`.
   */
  version: number;
  /**
   * The document's full current text.
   */
  text: string;
}
export interface LookupRequest {
  op: "lookup";
  language: "workshop" | "opy";
  query?: string | null;
  kind?:
    | "action"
    | "value"
    | "event"
    | "enumMember"
    | "enum"
    | "setting"
    | "parameter"
    | "settingPath"
    | "memberAction"
    | "memberValue"
    | "structural"
    | "operator"
    | null;
  within?: string | null;
  locale?: string | null;
  limit?: number | null;
}
export interface Capabilities {
  name: string;
  version: string;
  contract: "wright-result/v1";
  agent_contract: "wright-agent/v1";
  operations: string[];
  /**
   * The committed-schema definition describing each advertised operation's result (#532): operation name to its `#/$defs/<Op>Result` reference.
   */
  result_schemas: {
    [k: string]: string | undefined;
  };
  languages: string[];
  profiles: string[];
  [k: string]: unknown | undefined;
}
export interface WorkflowEnvelope {
  wright: ResultContract;
  command: string;
  ok: boolean;
  exit: number;
  diagnostics: Diagnostic[];
  result: {
    [k: string]: unknown | undefined;
  };
  [k: string]: unknown | undefined;
}
export interface ResultContract {
  version: string;
  contract: "wright-result/v1";
  [k: string]: unknown | undefined;
}
export interface Diagnostic {
  code: string;
  stage: "discovery" | "frontend" | "lowering" | "validation" | "emission" | "analysis" | "reconstruction" | "internal";
  severity: "error" | "warning" | "info";
  message: string;
  status?: "supported" | "partial" | "unsupported";
  span?: SourceSpan;
  source?: Origin;
  [k: string]: unknown | undefined;
}
export interface SourceSpan {
  file: number;
  path?: string;
  start: Position1Based;
  end: Position1Based;
  [k: string]: unknown | undefined;
}
export interface Position1Based {
  line: number;
  col: number;
  [k: string]: unknown | undefined;
}
export interface Origin {
  kind: string;
  locale?: string;
  [k: string]: unknown | undefined;
}
export interface CompilePayload {
  output?: {
    /**
     * SHA-256 of the written program.
     */
    sha256: string;
    locale: string;
    /**
     * The file the program was written to.
     */
    written_to: string;
    /**
     * The loaded input's primary entry source digest — same semantics as `project.inputIdentity`.
     */
    input_identity: string;
    [k: string]: unknown | undefined;
  };
  [k: string]: unknown | undefined;
}
export interface CheckPayload {
  [k: string]: unknown | undefined;
}
export interface AnalyzePayload {
  program: AnalyzedProgramSummary;
  cost: {
    [k: string]: unknown | undefined;
  };
  complexity: {
    [k: string]: unknown | undefined;
  };
  hotspots: unknown[];
  conditionTrees: unknown[];
  risks: ActionableFinding[];
  variables: unknown[];
  persistentObjects: {
    [k: string]: unknown | undefined;
  };
  counts: {
    [k: string]: unknown | undefined;
  };
  expand: string;
  truncation?: Truncation;
  selection?: SelectionSummary;
}
export interface AnalyzedProgramSummary {
  origin: Origin;
  files: number;
  globalVariables: number;
  playerVariables: number;
  subroutines: number;
  rules: number;
  [k: string]: unknown | undefined;
}
export interface ActionableFinding {
  code: string;
  severity: "error" | "warning" | "info";
  message: string;
  /**
   * Source path and 1-based line/column; full spans are available through findings.
   */
  location?: string;
  evidence: string;
  boundedness?: string;
}
export interface Truncation {
  [k: string]:
    | {
        total: number;
        shown: number;
        truncated: number;
        expand: string;
      }
    | undefined;
}
export interface SelectionSummary {
  total: number;
  withheld: number;
  [k: string]: unknown | undefined;
}
export interface InspectPayload {
  program: ProgramSummary;
  counts: {
    [k: string]: unknown | undefined;
  };
  rules: {
    id: number;
    name: string;
    [k: string]: unknown | undefined;
  }[];
  expand: string;
  truncation?: Truncation;
  selection?: SelectionSummary;
}
export interface ProgramSummary {
  origin: Origin;
  files: number;
  globalVariables: number;
  playerVariables: number;
  subroutines: number;
  rules: number;
  findings: number;
  [k: string]: unknown | undefined;
}
export interface ProjectResult {
  origin: Origin;
  /**
   * SHA-256 hex digest of the input's primary entry source (the provider's `sourceIdentity` for provider projects). For a single-file input it covers the whole input; for a provider directory project it hashes only the entry file, so member-file edits do not change it — use `stale-id` refusals for freshness, not this value.
   */
  inputIdentity: string;
  files: number;
  globalVariables: number;
  playerVariables: number;
  subroutines: number;
  rules: number;
  symbols: number;
  findings: number;
  [k: string]: unknown | undefined;
}
export interface RuleSummary {
  id: number;
  name: string;
  /**
   * The rule's event binding: a canonical event id, or `subroutine:<name>` for a subroutine rule.
   */
  event?: string;
  span: Span;
  /**
   * Present when `content` was requested: the owner's `workshop-rs/rule-content-v1` document. Each condition and action also carries `span` when the program has one.
   */
  content?: {
    format: "workshop-rs/rule-content-v1";
    event: {
      [k: string]: unknown | undefined;
    };
    conditions: {
      [k: string]: unknown | undefined;
    }[];
    actions: {
      [k: string]: unknown | undefined;
    }[];
    [k: string]: unknown | undefined;
  };
  [k: string]: unknown | undefined;
}
export interface RulesSelectionResult {
  rules: RuleSummary[];
  selection: SelectionSummary;
  /**
   * ADR-0024: result members the result should carry but cannot for this input. `names` names the owner that must serve target-language spellings (today `opy-rs` for an OverPy input, until its provider capability exists); raw Workshop results carry no `unavailable`.
   */
  unavailable?: {
    [k: string]: unknown | undefined;
  };
  [k: string]: unknown | undefined;
}
export interface CappedListResult {
  items: unknown[];
  truncation: Truncation;
}
export interface Symbol {
  id: number;
  kind: string;
  name: string;
  span: Span;
  [k: string]: unknown | undefined;
}
export interface SymbolsSelectionResult {
  symbols: Symbol[];
  selection: SelectionSummary;
  [k: string]: unknown | undefined;
}
export interface UsageResult {
  id: number;
  kind: string;
  symbol: string;
  reads: number;
  writes: number;
  calls: number;
  rules: number;
  [k: string]: unknown | undefined;
}
export interface Reference {
  kind: "declaration" | "definition" | "read" | "write" | "call";
  span: Span;
  rule: number | null;
  action: number | null;
  value: number | null;
  [k: string]: unknown | undefined;
}
export interface ReferencesSelectionResult {
  references: Reference[];
  selection: SelectionSummary;
  [k: string]: unknown | undefined;
}
export interface CfgResult {
  entry: number;
  exit: number;
  blocks: {
    id: number;
    kind: string;
    waits: boolean;
    calls: number[];
    actions: number[];
    successors: {
      to: number;
      kind: string;
      [k: string]: unknown | undefined;
    }[];
    [k: string]: unknown | undefined;
  }[];
  selection?: SelectionSummary;
  [k: string]: unknown | undefined;
}
export interface Finding {
  code: string;
  severity: "error" | "warning" | "info";
  message: string;
  span: Span;
  rule: number | null;
  action: number | null;
  value: number | null;
  evidence: string;
  boundedness: string | null;
  [k: string]: unknown | undefined;
}
export interface FindingsSelectionResult {
  findings: Finding[];
  selection: SelectionSummary;
  [k: string]: unknown | undefined;
}
export interface LintPayload {
  ok?: boolean;
  exit?: number;
  inputIdentity?: string;
  fixes?: unknown[];
  counts: {
    [k: string]: unknown | undefined;
  };
  findings: ActionableFinding[];
  expand: string;
  truncation?: Truncation;
  selection?: SelectionSummary;
}
export interface LintRulesResult {
  rules: LintRule[];
  config: LintConfiguration;
  skipped: SkippedRule[];
  [k: string]: unknown | undefined;
}
export interface LintRule {
  id: string;
  defaultSeverity: string;
  effectiveSeverity: string;
  enabled: boolean;
  summary: string;
  rationale: string;
  documentation: string;
  knownLimits: string;
  evidence: string;
  tags: string[];
  kind: string;
  [k: string]: unknown | undefined;
}
export interface LintConfiguration {
  rules: {
    [k: string]:
      | {
          [k: string]: unknown | undefined;
        }
      | undefined;
  };
  [k: string]: unknown | undefined;
}
export interface SkippedRule {
  id: string;
  rule: number;
  reason: string;
  [k: string]: unknown | undefined;
}
export interface CallGraphEdge {
  caller: string;
  callee: string;
  [k: string]: unknown | undefined;
}
export interface CallGraphSelectionResult {
  edges: CallGraphEdge[];
  selection: SelectionSummary;
  [k: string]: unknown | undefined;
}
export interface CostEstimateResult {
  exact: {
    emittedBytes: number;
    programActions: number;
    programRules: number;
    waitActions: number;
    [k: string]: unknown | undefined;
  };
  findings: {
    code: string;
    severity: string;
    message: string;
    [k: string]: unknown | undefined;
  }[];
  kind: {
    exact: string;
    findings: string;
    performance: string;
    [k: string]: unknown | undefined;
  };
  selection?: SelectionSummary;
  [k: string]: unknown | undefined;
}
export interface TargetMetadataResult {
  catalogVersion: string;
  locales: string[];
  actions: number;
  values: number;
  events: number;
  operators: number;
  enumDomains: {
    domain: string;
    members: string[];
    [k: string]: unknown | undefined;
  }[];
  [k: string]: unknown | undefined;
}
export interface EditValidationPayload {
  ok: boolean;
  exit: number;
  diagnostics: Diagnostic[];
  preview?: SourcePreview[];
  [k: string]: unknown | undefined;
}
export interface SourcePreview {
  source: string;
  source_identity: string;
  new_text: string;
  [k: string]: unknown | undefined;
}
export interface ProviderMutationPayload {
  ok: boolean;
  transaction?: EditTransaction;
  diagnostics: Diagnostic[];
  preview?: SourcePreview[];
  provider_code?: string;
  provider_message?: string;
  [k: string]: unknown | undefined;
}
export interface LookupResult {
  language: string;
  owner: string;
  entries: LookupEntry[];
  unavailable?: {
    capability: string;
    message: string;
    [k: string]: unknown | undefined;
  };
  [k: string]: unknown | undefined;
}
export interface LookupEntry {
  identity: string;
  kind: string;
  spelling: string;
  displayName: string;
  signature?: string;
  callable?: {
    /**
     * Optional: the owner-declared return type of a value, in the same vocabulary as parameter types.
     */
    returns?: string;
    /**
     * Optional: present as true when the callable's last parameter repeats for extra arguments.
     */
    variadic?: boolean;
    [k: string]: unknown | undefined;
  };
  enum?: {
    [k: string]: unknown | undefined;
  };
  parameter?: {
    [k: string]: unknown | undefined;
  };
  setting?: {
    [k: string]: unknown | undefined;
  };
  [k: string]: unknown | undefined;
}
export interface ToolErrorInfo {
  /**
   * Machine-readable refusal code. `stale-id` reports a numeric symbol id or rule index issued by an earlier program: after the loaded project changed on disk, the id must be re-read from a fresh `symbols`/`rules` listing or replaced by the declared name.
   */
  code: string;
  message: string;
  [k: string]: unknown | undefined;
}
