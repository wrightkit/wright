use std::collections::BTreeMap;

use super::{ToolErrorInfo, ToolResponse, ToolService};
use crate::CompilerSession;
use crate::result::{Envelope, SimulateResult, exit, exit_code_from, version_info};

pub(crate) const ATHENA_VERSION: &str = "0.5.0";

impl<'a> ToolService<'a> {
    pub(crate) fn simulate(
        &mut self,
        input_identity: &str,
        sources: &Option<BTreeMap<String, String>>,
        seconds: Option<f64>,
        scenario: &Option<serde_json::Value>,
    ) -> ToolResponse {
        let refuse = |code: &str, message: String| ToolResponse::Error {
            error: ToolErrorInfo::new(code, message),
        };
        let seconds = match (seconds, scenario) {
            (Some(_), Some(_)) => {
                return refuse(
                    "malformed-request",
                    "send `seconds` for a scenario-free run or a `scenario` (which owns the simulated time) — never both".to_string(),
                );
            }
            (Some(seconds), None) if !seconds.is_finite() || seconds <= 0.0 => {
                return refuse(
                    "malformed-request",
                    format!("`seconds` must be a positive finite number (got {seconds})"),
                );
            }
            (Some(seconds), None) => Some(seconds),
            (None, Some(_)) => None,
            (None, None) => {
                return refuse(
                    "malformed-request",
                    "a simulate run needs `seconds` or a `scenario`; without one there is no simulated time".to_string(),
                );
            }
        };
        let loaded = self.snapshot().loaded.clone();
        let (effective, _) = match self.derived_loaded(loaded, sources) {
            Ok(derived) => derived,
            Err(diagnostics) => {
                return ToolResponse::Error {
                    error: Self::loader_error(
                        diagnostics
                            .into_iter()
                            .next()
                            .expect("refusals carry one diagnostic"),
                    ),
                };
            }
        };
        if input_identity != effective.input.identity {
            return refuse(
                "edit-stale-source",
                "the request's inputIdentity does not match the program to simulate — re-read `project` (or pass the current `sources` text) and retry".to_string(),
            );
        }
        let locale = CompilerSession::locale_for(&effective);
        let text =
            match workshop_rs::emitter::emit(&effective.program, self.session.catalog(), &locale) {
                Ok(text) => text,
                Err(error) => return refuse("workshop-emission", error.to_string()),
            };
        let trace = match scenario {
            Some(scenario) => match athena_sim::run_scenario(&text, &scenario.to_string()) {
                Ok(trace) => trace,
                Err(error) => {
                    return match error {
                        athena_sim::RunError::Scenario(error) => {
                            refuse("malformed-scenario", error.to_string())
                        }
                        athena_sim::RunError::Parse(error) => {
                            refuse("simulate-parse", error.to_string())
                        }
                        athena_sim::RunError::HeroValues(error) => {
                            refuse("simulate-hero-values", error.to_string())
                        }
                    };
                }
            },
            None => match athena_sim::run_text(
                &text,
                athena_sim::Options {
                    seconds: seconds.expect("required without a scenario"),
                },
            ) {
                Ok(trace) => trace,
                Err(error) => return refuse("simulate-parse", error.to_string()),
            },
        };
        let diagnostics = (*effective.load_diagnostics).clone();
        let exit_code = exit_code_from(&diagnostics);
        self.ok(serde_json::to_value(Envelope {
            wright: version_info(),
            command: "simulate".to_string(),
            ok: exit_code == exit::SUCCESS,
            exit: exit_code,
            diagnostics,
            selection: None,
            gated: false,
            result: SimulateResult { trace },
            comparison: None,
        })
        .expect("serializes"))
    }
}
