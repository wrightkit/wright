use serde::Serialize;

use super::{AGENT_CONTRACT, SERVICE_NAME, SERVICE_VERSION, ToolService};
use crate::RESULT_CONTRACT;

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
    "editStatements",
    "lookup",
    "listEntries",
    "simulate",
];

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
    /// The `athena` release `simulate` links against (ADR-0033) — the owner
    /// of the trace format it returns.
    pub athena_version: String,
}

impl<'a> ToolService<'a> {
    /// The capability/version contract.
    pub fn capabilities(&self) -> Capabilities {
        let operations: Vec<String> = OPERATIONS.iter().map(|op| op.to_string()).collect();
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
            athena_version: super::simulate::ATHENA_VERSION.to_string(),
        }
    }
}
