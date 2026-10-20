//! The error contract for declarative rule loading and validation.

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
        }
    }
}

impl std::error::Error for RuleError {}
