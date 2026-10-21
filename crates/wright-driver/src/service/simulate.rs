//! `simulate` (ADR-0033): a thin pass to `athena`'s simulator entry points
//! over the effective program's canonical Workshop text — the served
//! snapshot's program emitted exactly as `wright compile` writes it (a
//! provider-loaded source project's artifact included), or the caller's
//! `sources` text loaded like `editStatements` does — returning the trace
//! unchanged inside a `wright-result/v1` envelope. `athena` owns execution
//! semantics and the trace contract (`complete`, `not_modeled`,
//! `missing_input` are all result content); Wright owns the freshness
//! precondition, the `sources` binding shared with the edit operations,
//! and the refusal channel for a request that cannot be served at all.

use std::collections::BTreeMap;

use super::{ToolErrorInfo, ToolResponse, ToolService};
use crate::CompilerSession;
use crate::result::{Envelope, SimulateResult, exit, exit_code_from, version_info};
use crate::statement_edit::supplied_source;

/// The `athena` release `simulate` links against, reported by
/// `capabilities` as `athena_version` (ADR-0033). It names the pinned
/// `athena-sim` tag in `wright-driver`'s dependency — keep them equal.
pub(crate) const ATHENA_VERSION: &str = "0.2.0";

impl<'a> ToolService<'a> {
    /// `simulate` (ADR-0033): run the effective program under `athena` and
    /// return its trace inside a `wright-result/v1` envelope. The request
    /// is refused before anything runs when the simulated time is missing
    /// or named twice, the `sources` carry no text for the loaded input,
    /// the `inputIdentity` is stale, or the caller's text does not load.
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
        let refuse = |code: &str, message: String| ToolErrorInfo {
            code: code.to_string(),
            message,
        };
        // `seconds` is `athena`'s `Options.seconds` for a scenario-free run;
        // a scenario owns the simulated time, so both spellings at once are
        // two sources of truth — refused, not arbitrated — and neither is a
        // run with no simulated time at all.
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
        // The program to simulate is the caller's `sources` text when
        // supplied — loaded through the session's own Workshop path exactly
        // like `editStatements` — else the served snapshot; `inputIdentity`
        // checks against whichever program that is.
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
        // `athena` reads Workshop text and no `Program`, so callers may pin
        // another workshop-rs; the effective program's canonical emission —
        // the same text `wright compile` writes — is the faithful form, so
        // a provider-loaded source-language project simulates its compiled
        // artifact rather than being refused at a kind boundary.
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
