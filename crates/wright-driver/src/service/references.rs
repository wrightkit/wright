//! The `references` operation: a symbol's references, each carrying its
//! source excerpt (#1099), under the request's selection.

use super::*;

impl ToolService<'_> {
    pub(super) fn references(
        &self,
        symbol: &Address,
        kind: Option<&str>,
        rule: Option<&Address>,
        file: Option<&str>,
        max: Option<usize>,
    ) -> ToolResponse {
        if let Some(kind) = kind
            && !ReferenceKind::ALL
                .iter()
                .any(|known| known.as_str() == kind)
        {
            return invalid_selection(format!(
                "unknown reference kind '{}' (expected one of: {})",
                kind,
                ReferenceKind::ALL
                    .iter()
                    .map(|known| known.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let rule_id = match rule {
            Some(address) => match self.rule_address(address) {
                Ok(id) if (id as usize) < self.snapshot().loaded.program.rules.len() => Some(id),
                Ok(id) => return invalid_selection(format!("unknown rule {id}")),
                Err(error) => {
                    return invalid_selection_fields(error.message, error.fields);
                }
            },
            None => None,
        };
        match self.symbol_address(symbol) {
            Ok(symbol) => {
                match self
                    .semantic_query_with_resolved_span_paths(Request::FindReferences { symbol })
                {
                    ToolResponse::Ok { mut result } => {
                        self.attach_excerpts(symbol, &mut result);
                        self.select_result(
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
                        )
                    }
                    other => other,
                }
            }
            Err(error) => ToolResponse::Error { error },
        }
    }

    fn attach_excerpts(&self, symbol: u32, references: &mut serde_json::Value) {
        if let ToolResponse::Ok { result } = self.semantic_query(Request::GetSymbol { symbol })
            && let Some(name) = result["name"].as_str()
        {
            crate::excerpt::attach_excerpts(references, name, &self.snapshot().loaded);
        }
    }
}
