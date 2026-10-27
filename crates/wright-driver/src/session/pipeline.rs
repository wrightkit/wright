//! The shared workflow tail (#1002): every operation resolves its
//! `Envelope` through `finish`, so the verdict — diagnostics, selection,
//! and the configured `--fail-on` gate — is decided in exactly one place
//! for CLI, `serve`, MCP, and embedding callers.

use super::{CompilerSession, Loaded};
use crate::diag::Diagnostic;
use crate::result::{Envelope, version_info};

impl CompilerSession {
    pub(crate) fn finish<T: serde::Serialize>(&mut self, command: &str, result: T) -> Envelope<T> {
        let diagnostics = std::mem::take(&mut self.diagnostics);
        let (diagnostics, selection, exit) =
            crate::select::envelope_diagnostics(&self.config, self.loaded.as_ref(), diagnostics);
        let mut envelope = Envelope {
            wright: version_info(),
            command: command.to_string(),
            ok: exit == crate::result::exit::SUCCESS,
            exit,
            diagnostics,
            selection,
            gated: false,
            result,
            comparison: None,
        };
        if let Some(threshold) = self.config.fail_on {
            envelope.apply_fail_on(threshold, None);
        }
        envelope
    }

    pub(crate) fn with_loaded<T>(
        &mut self,
        command: &str,
        load: impl FnOnce(&mut Self) -> Result<Loaded, Diagnostic>,
        run: impl FnOnce(&mut Self, Loaded) -> T,
    ) -> Envelope<T>
    where
        T: Default + serde::Serialize,
    {
        let result = match self.verify_fixed_config().and_then(|()| load(self)) {
            Ok(loaded) => run(self, loaded),
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                T::default()
            }
        };
        self.finish(command, result)
    }
}
