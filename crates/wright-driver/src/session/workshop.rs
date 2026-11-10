//! The Workshop-text load: detect the locale, then parse, canonicalize, and
//! validate a resolved Workshop input. Split from `session.rs` to keep that
//! file under the size budget.

use workshop_rs::Program;

use super::CompilerSession;
use crate::diag::{Diagnostic, workshop_diag};
use crate::input::ResolvedInput;
use crate::progress::{ProgressEvent, ProgressPhase};

impl CompilerSession {
    #[allow(unsafe_code)] // hotpath 0.28 emits `#[unsafe(export_name)]` statics
    pub(super) fn load_workshop(
        &mut self,
        resolved: &ResolvedInput,
    ) -> Result<(Program, String), Diagnostic> {
        let override_locale = self
            .config
            .locale
            .as_deref()
            .map(workshop_rs::catalog::Locale::new);
        let locale = hotpath::measure_block!("load::detect_locale", {
            workshop_rs::detect::resolve_locale(
                &resolved.text,
                self.catalog,
                override_locale.as_ref(),
            )
        })
        .map_err(|error| workshop_diag(error, resolved))?;
        let program = self.trace.in_span("parse", || {
            hotpath::measure_block!("load::parse", {
                workshop_rs::parser::parse_with_context(
                    &resolved.text,
                    self.catalog,
                    &locale,
                    self.catalog,
                )
            })
            .map_err(|error| workshop_diag(error, resolved))
        })?;
        self.progress(ProgressEvent::new(ProgressPhase::Validation));
        let canonical = self.trace.in_span("canonical", || {
            hotpath::measure_block!("load::canonical", program.canonical())
                .map_err(|error| workshop_diag(error, resolved))
        })?;
        self.trace.in_span("validate", || {
            hotpath::measure_block!("load::validate", canonical.validate())
                .map_err(|error| workshop_diag(error, resolved))?;
            hotpath::measure_block!(
                "load::validate_canonical",
                canonical.validate_canonical_ids_tolerating_residuals(self.catalog)
            )
            .map_err(|error| workshop_diag(error, resolved))
        })?;
        drop(canonical);
        Ok((program, locale.to_string()))
    }
}
