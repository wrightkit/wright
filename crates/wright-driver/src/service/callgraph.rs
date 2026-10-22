//! The `callGraph` operation: caller rules → callee subroutines.

use serde_json::json;
use wright_analyzer::service::marker;

use super::{ToolResponse, ToolService, invalid_selection};

impl<'a> ToolService<'a> {
    /// `callGraph`: caller rules → callee subroutines. The selection fields
    /// are `caller` (one rule address, ADR-0035), `callee` (a declared
    /// subroutine name), and `max` (#531). `caller` is a filter: an
    /// unmatched or ambiguous address is `invalid-selection` carrying the
    /// shared resolver's candidate list, never a merge of same-name rules.
    pub(crate) fn call_graph(
        &self,
        caller: Option<&str>,
        callee: Option<&str>,
        max: Option<usize>,
    ) -> ToolResponse {
        let program = &self.snapshot().loaded.program;
        let caller_address = match caller {
            Some(caller) => match self.semantic().resolve_rule(caller) {
                Ok(rule) => Some(self.semantic().canonical_rule_address(rule)),
                Err(error) => return invalid_selection(error.message),
            },
            None => None,
        };
        if let Some(callee) = callee
            && !program
                .subroutines
                .iter()
                .any(|subroutine| subroutine.name == callee)
        {
            return invalid_selection(format!(
                "unknown callee subroutine '{}'",
                marker::escape(callee)
            ));
        }
        let edges = program
            .rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| !rule.disabled)
            .flat_map(|(id, rule)| {
                wright_analyzer::canonical::live_actions(&rule.actions).filter_map(
                    move |(_, action)| {
                        wright_analyzer::canonical::subroutine_callee(action).map(|callee| {
                            json!({ "caller": rule.name, "callerAddress": self.semantic().canonical_rule_address(id), "callee": callee })
                        })
                    },
                )
            })
            .collect::<Vec<_>>();
        self.select_result(
            "edges",
            serde_json::Value::Array(edges),
            None,
            |edge| {
                caller_address
                    .as_deref()
                    .is_none_or(|caller| edge["callerAddress"].as_str() == Some(caller))
                    && callee.is_none_or(|callee| edge["callee"].as_str() == Some(callee))
            },
            max,
            caller.is_some() || callee.is_some() || max.is_some(),
        )
    }
}
