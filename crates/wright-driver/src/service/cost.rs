//! The `costEstimate` operation's whole-program and per-rule payloads plus
//! the CLI-side run-cache comparison and `--max-total` gate
//! (ADR-0027/0030).

use serde_json::json;
use wright_analyzer::service::marker;
use wright_analyzer::service::{Request, Response};

use super::{ToolErrorInfo, ToolResponse, ToolService};
use crate::{CompilerSession, Loaded};

impl<'a> ToolService<'a> {
    /// `costEstimate`: exact generated-resource counts plus static findings.
    /// Findings and skipped rule evaluations come from the same configured
    /// semantic service `lint` serves (#788); the findings subset honors
    /// the shared selection (#430) and findings carry resolved span paths
    /// like `lint` (#722), so a `file` selection can match them. The `rule`
    /// selector (#724) reports one declared rule's element-cost breakdown
    /// instead of the whole-program view.
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
        result["snapshot"] = json!(self.generation);
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
            && total as usize > max
        {
            return Err(ToolErrorInfo {
                code: "cost-over-budget".to_string(),
                message: format!(
                    "element total {total} exceeds --max-total {max} by {}",
                    total as usize - max
                ),
            });
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
                    marker::escape(run.baseline.as_deref().unwrap_or_default())
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
