//! The generation-scoped numeric-address guard (#595): `symbol`/`rule`
//! numeric ids are valid only for the program generation that issued
//! them.

use super::{Address, ToolErrorInfo, ToolRequest, ToolService};

impl ToolService<'_> {
    /// The snapshot gate for numeric addresses (#595): every numeric
    /// symbol/rule address a request carries must be scoped to the served
    /// program's generation — the request's `snapshot` member must match
    /// `self.generation`. A missing token or a different generation is
    /// refused `stale-id`, so a numeric id captured from an earlier program
    /// can never silently resolve against a reloaded or recovered one. A
    /// `references` request's numeric `rule` filter is checked like its
    /// `symbol` (#592). A request whose addresses are all declared names is
    /// generation-agnostic: it ignores `snapshot` and resolves against the
    /// current program.
    pub(super) fn stale_id(&self, request: &ToolRequest) -> Option<ToolErrorInfo> {
        let (numeric, snapshot) = match request {
            ToolRequest::References {
                symbol,
                rule,
                snapshot,
                ..
            } => (
                matches!(symbol, Address::Id(_)) || matches!(rule, Some(Address::Id(_))),
                *snapshot,
            ),
            ToolRequest::Usage { symbol, snapshot } => {
                (matches!(symbol, Address::Id(_)), *snapshot)
            }
            ToolRequest::Cfg { rule, snapshot, .. } => (matches!(rule, Address::Id(_)), *snapshot),
            ToolRequest::Rules { rule, snapshot, .. } => {
                (matches!(rule, Some(Address::Id(_))), *snapshot)
            }
            ToolRequest::EditStatements {
                edits, snapshot, ..
            } => (
                edits.iter().any(|edit| matches!(edit.rule, Address::Id(_))),
                *snapshot,
            ),
            _ => (false, None),
        };
        if !numeric {
            return None;
        }
        match snapshot {
            Some(issued) if issued == self.generation => None,
            Some(issued) => Some(ToolErrorInfo::new(
                "stale-id",
                format!(
                    "numeric symbol ids and rule indexes are scoped to the program generation that issued them: the served program is snapshot {} but this request's addresses were issued under snapshot {issued} — re-issue the ids from a current `symbols`/`rules` (or other id-reporting) response, or address by name",
                    self.generation
                ),
            )),
            None => Some(ToolErrorInfo::new(
                "stale-id",
                format!(
                    "a numeric `symbol`/`rule` address is scoped to the program generation that issued it: send the `snapshot` value an id-reporting response (`symbols`, `rules`, `inspect`, `project`, …) returned — the current generation is {}",
                    self.generation
                ),
            )),
        }
    }
}
