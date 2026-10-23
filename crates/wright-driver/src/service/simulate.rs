use std::collections::BTreeMap;

use super::{ToolErrorInfo, ToolResponse, ToolService};
use crate::CompilerSession;
use crate::result::{Envelope, SimulateResult, exit, exit_code_from, version_info};
use crate::statement_edit::supplied_source;

pub(crate) const ATHENA_VERSION: &str = "0.2.0";

impl<'a> ToolService<'a> {
    pub(crate) fn simulate(
        &mut self,
        input_identity: &str,
        sources: &Option<BTreeMap<String, String>>,
        seconds: Option<f64>,
        scenario: &Option<serde_json::Value>,
    ) -> ToolResponse {
        match self.simulate_inner(input_identity, sources, seconds, scenario) {
            Ok(envelope) => self.ok(serde_json::to_value(envelope).expect("serializes")),
            Err(error) => ToolResponse::Error { error },
        }
    }

    fn simulate_inner(
        &mut self,
        input_identity: &str,
        sources: &Option<BTreeMap<String, String>>,
        seconds: Option<f64>,
        scenario: &Option<serde_json::Value>,
    ) -> Result<Envelope<SimulateResult>, ToolErrorInfo> {
        let refuse = |code: &str, message: String| ToolErrorInfo::new(code, message);
        let seconds = match (seconds, scenario) {
            (Some(_), Some(_)) => {
                return Err(refuse(
                    "malformed-request",
                    "send `seconds` for a scenario-free run or a `scenario` (which owns the simulated time) — never both".to_string(),
                ));
            }
            (Some(seconds), None) if !seconds.is_finite() || seconds <= 0.0 => {
                return Err(refuse(
                    "malformed-request",
                    format!("`seconds` must be a positive finite number (got {seconds})"),
                ));
            }
            (Some(seconds), None) => Some(seconds),
            (None, Some(_)) => None,
            (None, None) => {
                return Err(refuse(
                    "malformed-request",
                    "a simulate run needs `seconds` or a `scenario`; without one there is no simulated time".to_string(),
                ));
            }
        };
        let loaded = self.snapshot().loaded.clone();
        let derived;
        let effective = match sources {
            Some(sources) => {
                let Some((_key, text)) = supplied_source(sources, &loaded.input) else {
                    return Err(refuse(
                        "edit-unknown-source",
                        format!(
                            "the request's 'sources' carries no text for the loaded input '{}'",
                            loaded.input.display
                        ),
                    ));
                };
                let mut resolved = loaded.input.clone();
                resolved.text = text.to_string();
                resolved.identity = crate::input_identity(text);
                derived = match self.session.load_workshop_text(&resolved) {
                    Ok(derived) => derived,
                    Err(diagnostic) => return Err(Self::loader_error(diagnostic)),
                };
                &derived
            }
            None => &loaded,
        };
        if input_identity != effective.input.identity {
            return Err(refuse(
                "edit-stale-source",
                "the request's inputIdentity does not match the program to simulate — re-read `project` (or pass the current `sources` text) and retry".to_string(),
            ));
        }
        let locale = CompilerSession::locale_for(effective);
        let text = workshop_rs::emitter::emit(&effective.program, self.session.catalog(), &locale)
            .map_err(|error| refuse("workshop-emission", error.to_string()))?;
        let trace = match scenario {
            Some(scenario) => athena_sim::run_scenario(&text, &scenario.to_string()).map_err(
                |error| match error {
                    athena_sim::RunError::Scenario(error) => {
                        refuse("malformed-scenario", error.to_string())
                    }
                    athena_sim::RunError::Parse(error) => {
                        refuse("simulate-parse", error.to_string())
                    }
                    athena_sim::RunError::HeroValues(error) => {
                        refuse("simulate-hero-values", error.to_string())
                    }
                },
            )?,
            None => athena_sim::run_text(
                &text,
                athena_sim::Options {
                    seconds: seconds.expect("required without a scenario"),
                },
            )
            .map_err(|error| refuse("simulate-parse", error.to_string()))?,
        };
        let diagnostics = (*effective.load_diagnostics).clone();
        let exit_code = exit_code_from(&diagnostics);
        Ok(Envelope {
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
    }
}
