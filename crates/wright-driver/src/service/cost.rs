use serde_json::json;
use wright_analyzer::service::{Request, Response};

use super::{ToolErrorInfo, ToolResponse, ToolService};
use crate::{CompilerSession, Loaded};

impl<'a> ToolService<'a> {
    pub(crate) fn cost_estimate(
        &self,
        selection: &crate::select::FindingSelection,
        rule: Option<&str>,
    ) -> ToolResponse {
        match self.cost_estimate_result(selection, rule) {
            Ok((result, _)) => self.ok(result),
            Err(error) => ToolResponse::Error { error },
        }
    }

    pub(crate) fn cost_estimate_result(
        &self,
        selection: &crate::select::FindingSelection,
        rule: Option<&str>,
    ) -> Result<(serde_json::Value, Option<crate::select::SelectionOutcome>), ToolErrorInfo> {
        self.selection_check(selection)?;
        let loaded = &self.snapshot().loaded;
        if let Some(rule) = rule {
            if selection.is_active() {
                return Err(ToolErrorInfo::new(
                    "invalid-selection",
                    "the 'rule' selector reports one rule's element cost; finding selection fields do not apply",
                ));
            }
            return self.cost_estimate_rule(loaded, rule).map(|mut result| {
                result["snapshot"] = json!(self.generation);
                (result, None)
            });
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
        let service = self.configured_semantic();
        let findings = match service.handle(&Request::GetFindings) {
            Response::Ok { result } => result,
            Response::Error { .. } => serde_json::json!([]),
        };
        let findings =
            crate::session::prepare_findings(findings, service, loaded, self.session.catalog());
        let lint_rules = match service.handle(&Request::LintRules) {
            Response::Ok { result } => result,
            Response::Error { .. } => serde_json::json!({}),
        };
        let (_, _, skipped) = crate::result::split_lint_rules(&lint_rules);
        let (findings, outcome) =
            selection.apply_findings(findings, &crate::select::file_bases(&loaded.input));
        let mut exact = json!({
            "programActions": program_actions,
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
        match loaded.program.element_count(self.session.catalog()) {
            Ok(report) => {
                let mut rules: Vec<_> = report.rules.iter().enumerate().collect();
                rules.sort_by_key(|(index, node)| (std::cmp::Reverse(node.count), *index));
                let rule_entries: Vec<_> = rules
                    .iter()
                    .map(|(index, node)| {
                        let mut entry = json!({
                            "index": index,
                            "name": loaded.program.rules[*index].name,
                            "address": self.semantic().canonical_rule_address(*index),
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
                result["elementCount"] = json!({
                    "budget": crate::session::WORKSHOP_CLIENT_ELEMENT_LIMIT,
                });
                result["elementCountUnavailable"] = json!(error.to_string());
            }
        }
        if let Some(outcome) = &outcome {
            result["selection"] = serde_json::to_value(outcome).expect("selection serializes");
        }
        result["snapshot"] = json!(self.generation);
        Ok((result, outcome))
    }

    pub(crate) fn apply_cost_run(
        &self,
        result: &serde_json::Value,
    ) -> Result<Option<crate::cache::Comparison>, ToolErrorInfo> {
        let loaded = &self.snapshot().loaded;
        let run = &self.session.config.run;
        if let (Some(max), Some(total)) = (run.max_total, result["elementCount"]["total"].as_u64())
            && total as usize > max
        {
            return Err(ToolErrorInfo::new(
                "cost-over-budget",
                format!(
                    "element total {total} exceeds --max-total {max} by {}",
                    total as usize - max
                ),
            ));
        }
        let cost_run = self.session.cost_run(loaded);
        let cache_environment = crate::cache::CacheEnvironment::from_env();
        if run.baseline.is_some()
            && !cache_environment.ci
            && !crate::cache::baseline_exists_in(&cost_run, cache_environment)
        {
            return Err(ToolErrorInfo::new(
                "invalid-selection",
                format!(
                    "unknown baseline '{}': no record under .wright/ for this input kind/locale/profile",
                    run.baseline.as_deref().unwrap_or_default()
                ),
            ));
        }
        let Some(snapshot) = self.session.cost_snapshot(loaded) else {
            return Ok(None);
        };
        Ok(crate::cache::compare_and_record_in(
            &cost_run,
            &snapshot,
            cache_environment,
        ))
    }

    fn cost_estimate_rule(
        &self,
        loaded: &Loaded,
        rule: &str,
    ) -> Result<serde_json::Value, ToolErrorInfo> {
        let rule_id = self
            .semantic()
            .resolve_rule(rule)
            .map_err(|error| ToolErrorInfo {
                fields: error.fields,
                ..ToolErrorInfo::new("invalid-selection", error.message)
            })?;
        let report = loaded
            .program
            .element_count(self.session.catalog())
            .map_err(|error| ToolErrorInfo::new("element-count-unavailable", error.to_string()))?;
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
                "address": self.semantic().canonical_rule_address(rule_id),
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
}
