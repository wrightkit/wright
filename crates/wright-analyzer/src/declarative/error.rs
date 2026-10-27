use std::fmt;

#[derive(Debug)]
pub enum RuleError {
    Yaml(yaml_serde::Error),
    InvalidIdentity(String),
    InvalidFirstPartyIdentity(String),
    UnsupportedLocale(String),
    UnknownSpelling {
        kind: &'static str,
        spelling: String,
        locale: String,
    },
    UnknownParameter {
        action: String,
        parameter: String,
        locale: String,
    },
    NamedParameterRequiresAction,
    AmbiguousValuePattern,
    ComparisonNeedsNumber,
    /// A call pattern must carry exactly one of `name` or `names` (#970).
    AmbiguousCallName,
    CallNeedsName,
    /// `matcher.scope` listed no scopes (#970).
    EmptyScopeList,
    /// `report: each-match` has nothing to report without `values` (#970).
    EachMatchNeedsValues,
    /// `within` is only meaningful inside the ancestor-aware `values`
    /// search (#970).
    WithinOutsideValues,
    /// `evidence`/`severity` metadata are reserved to first-party packages
    /// (#970, SPEC-309).
    FirstPartyMetadataField {
        field: &'static str,
    },
    /// A first-party rule may declare `severity: error` only when its
    /// evidence class is `runtime-validated` — the audit contract (#1010,
    /// `docs/agent-contract.md`) reserves `error` for findings confirmed by
    /// runtime evaluation, because any error-severity finding yields
    /// `ok: false` and `exit: 1`.
    UnjustifiedErrorSeverity,
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleError::Yaml(err) => write!(f, "invalid rule YAML: {err}"),
            RuleError::InvalidIdentity(id) => write!(
                f,
                "invalid rule ID '{id}': must be '<namespace>/<id>' and '<namespace>' cannot be 'wright'"
            ),
            RuleError::InvalidFirstPartyIdentity(id) => write!(
                f,
                "invalid first-party rule ID '{id}': must be a bare ID or '<id>' under the reserved 'wright' namespace"
            ),
            RuleError::UnsupportedLocale(locale) => {
                write!(f, "unsupported catalog locale: {locale}")
            }
            RuleError::UnknownSpelling {
                kind,
                spelling,
                locale,
            } => write!(f, "unknown {kind} '{spelling}' for locale {locale}"),
            RuleError::UnknownParameter {
                action,
                parameter,
                locale,
            } => write!(
                f,
                "unknown parameter '{parameter}' for action '{action}' in locale {locale}"
            ),
            RuleError::NamedParameterRequiresAction => {
                write!(f, "parameter matchers require an exact action name")
            }
            RuleError::AmbiguousValuePattern => {
                write!(f, "value pattern specifies more than one pattern kind")
            }
            RuleError::ComparisonNeedsNumber => {
                write!(f, "comparison pattern value must be a number pattern")
            }
            RuleError::AmbiguousCallName => {
                write!(f, "call pattern must not carry both 'name' and 'names'")
            }
            RuleError::CallNeedsName => {
                write!(f, "call pattern requires 'name' or 'names'")
            }
            RuleError::EmptyScopeList => {
                write!(f, "matcher scope list must not be empty")
            }
            RuleError::EachMatchNeedsValues => {
                write!(f, "report 'each-match' requires a 'values' matcher")
            }
            RuleError::WithinOutsideValues => {
                write!(f, "'within' is only allowed on a 'values' pattern")
            }
            RuleError::FirstPartyMetadataField { field } => {
                write!(
                    f,
                    "metadata field '{field}' is only allowed in first-party rule packages"
                )
            }
            RuleError::UnjustifiedErrorSeverity => {
                write!(
                    f,
                    "`severity: error` requires `evidence: runtime-validated`"
                )
            }
        }
    }
}

impl std::error::Error for RuleError {}
