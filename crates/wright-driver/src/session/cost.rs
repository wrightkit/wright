//! Element-count cost accounting for a loaded program: the client
//! import-limit diagnostic, the cost snapshot, and the cost-cache
//! comparison. Split from `session.rs` so the session loader stays under
//! the context-size limit.

use super::{CompilerSession, Loaded, WORKSHOP_CLIENT_ELEMENT_LIMIT, loaded_origin, semantic};
use crate::cache;
use crate::diag::{Diagnostic, Severity, Stage, class_for_stage, source_span};

impl CompilerSession {
    pub(crate) fn cost_snapshot(&self, loaded: &Loaded) -> Option<cache::CostSnapshot> {
        let report = loaded.program.element_count(self.catalog).ok()?;
        Some(Self::snapshot_of(&report, loaded))
    }

    fn snapshot_of(
        report: &workshop_rs::actions::ElementCountReport,
        loaded: &Loaded,
    ) -> cache::CostSnapshot {
        cache::CostSnapshot {
            total: report.total,
            budget: WORKSHOP_CLIENT_ELEMENT_LIMIT,
            rules: report
                .rules
                .iter()
                .enumerate()
                .map(|(index, node)| (loaded.program.rules[index].name.clone(), node.count))
                .collect(),
        }
    }

    pub(crate) fn cost_run(&self, loaded: &Loaded) -> cache::CostRun {
        let run = &self.config.run;
        cache::CostRun {
            no_cache: run.no_cache,
            root: loaded.input.root.clone(),
            input_path: loaded.input.path.clone(),
            kind: loaded.input.kind.as_str().to_string(),
            locale: Self::locale_for(loaded).to_string(),
            profile: self.config.profile.as_str().to_string(),
            baseline: run.baseline.clone(),
            save_baseline: run.save_baseline.clone(),
        }
    }

    pub(crate) fn run_cost_comparison(
        &self,
        snapshot: Option<cache::CostSnapshot>,
    ) -> Option<cache::Comparison> {
        let loaded = self.loaded.as_ref()?;
        let run = self.cost_run(loaded);
        let environment = cache::CacheEnvironment::from_env();
        if run.baseline.is_some() && !cache::baseline_exists_in(&run, environment) {
            return None;
        }
        cache::compare_and_record_in(&run, &snapshot?, environment)
    }

    pub(crate) fn baseline_error(&self) -> Option<String> {
        let run = self.cost_run(self.loaded.as_ref()?);
        cache::baseline_error(&run, cache::CacheEnvironment::from_env())
    }

    #[allow(unsafe_code)] // hotpath 0.28 emits `#[unsafe(export_name)]` statics
    pub(super) fn attach_target_limits(&mut self, loaded: &Loaded) -> Option<cache::CostSnapshot> {
        let report = match hotpath::measure_block!("compile::element_count", {
            loaded.program.element_count(self.catalog)
        }) {
            Ok(report) => report,
            Err(error) => {
                self.diagnostics.push(Diagnostic {
                    code: "element-count-unavailable".to_string(),
                    stage: Stage::Emission,
                    severity: Severity::Info,
                    class: class_for_stage(Stage::Emission),
                    message: format!(
                        "the canonical element count is unavailable ({}); the {WORKSHOP_CLIENT_ELEMENT_LIMIT}-element client import limit was not evaluated",
                        error
                    ),
                    status: None,
                    span: None,
                    source: Some(loaded_origin(loaded)),
                    fields: Default::default(),
                });
                return None;
            }
        };
        let snapshot = Self::snapshot_of(&report, loaded);
        if report.total <= WORKSHOP_CLIENT_ELEMENT_LIMIT {
            return Some(snapshot);
        }
        let total = report.total;
        let mut diagnostic = Diagnostic {
            code: "target-element-limit".to_string(),
            stage: Stage::Emission,
            severity: Severity::Warning,
            class: class_for_stage(Stage::Emission),
            message: format!(
                "generated Workshop program uses {total} elements, over the {WORKSHOP_CLIENT_ELEMENT_LIMIT}-element client import limit; the artifact was still emitted but is expected to fail Overwatch client import"
            ),
            status: None,
            span: None,
            source: Some(loaded_origin(loaded)),
            fields: Default::default(),
        };
        if let Some(top) = report.rules.iter().max_by_key(|rule| rule.count) {
            diagnostic.message.push_str(&format!(
                "; largest contributor: rule \"{}\" ({} elements)",
                top.name, top.count
            ));
            diagnostic.span = top.span.map(|span| {
                source_span(
                    span,
                    semantic::span_path(Some(span.file.index() as u64), loaded),
                )
            });
        }
        self.diagnostics.push(diagnostic);
        Some(snapshot)
    }
}
