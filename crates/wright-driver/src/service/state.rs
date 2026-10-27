//! The service's carried state: the loaded `Snapshot` every response is
//! stamped from, and `ServiceState` — the form a host holds between
//! requests when it cannot keep a `ToolService` borrow alive (#959).

use std::sync::Arc;

use wright_analyzer::canonical::SemanticService;

use super::ToolService;
use crate::diag::Diagnostic;
use crate::input;
use crate::{CompilerSession, Loaded};

pub(super) struct Snapshot {
    pub(super) loaded: Loaded,
    pub(super) semantic: Arc<SemanticService<'static>>,
    pub(super) lint_semantic: Option<SemanticService<'static>>,
    pub(super) fingerprint: input::DiskFingerprint,
}

/// The state a host carries between requests when it cannot hold a
/// `ToolService` borrow across calls (#959): the WebAssembly entry points
/// own the session and resume a fresh service per request, and this state
/// keeps the snapshot generation, the reload count, and the loaded
/// snapshot itself continuous — a resumed service answers the same
/// `snapshot` values a borrowed one would. Constructed by
/// [`ToolService::into_state`], restored by [`ToolService::resume`].
pub struct ServiceState {
    pub(super) snapshot: Option<Snapshot>,
    pub(super) generation: u64,
    pub(super) reloads: usize,
    pub(super) invalidated: bool,
}

impl<'a> ToolService<'a> {
    /// Resume a service over `session` carrying `state` (#959): hosts that
    /// cannot borrow the session across calls — the WebAssembly entry
    /// points — restore the snapshot, generation, and reload bookkeeping
    /// verbatim instead of re-deriving them, so numeric `snapshot`
    /// addresses keep their meaning across the resume boundary.
    pub fn resume(
        session: &'a mut CompilerSession,
        state: ServiceState,
    ) -> Result<ToolService<'a>, Diagnostic> {
        session.verify_fixed_config()?;
        Ok(ToolService {
            session,
            snapshot: state.snapshot,
            generation: state.generation,
            reloads: state.reloads,
            invalidated: state.invalidated,
            selected_meta: Vec::new(),
        })
    }

    /// Hand the service's carried state back to the host; the next call's
    /// [`ToolService::resume`] restores it.
    pub fn into_state(self) -> ServiceState {
        ServiceState {
            snapshot: self.snapshot,
            generation: self.generation,
            reloads: self.reloads,
            invalidated: self.invalidated,
        }
    }

    pub(super) fn refresh(&mut self) -> Result<(), super::ToolErrorInfo> {
        let Some(snapshot) = &self.snapshot else {
            let loaded = self.session.load().map_err(Self::loader_error)?;
            self.adopt(loaded);
            if self.invalidated {
                self.reloads += 1;
                self.invalidated = false;
            }
            return Ok(());
        };
        let fingerprint = input::disk_fingerprint(&self.session.config, &snapshot.loaded.input);
        if snapshot.fingerprint == fingerprint {
            return Ok(());
        }
        let loaded = match self.session.reload() {
            Ok(loaded) => loaded,
            Err(diagnostic) => {
                self.invalidate();
                return Err(Self::loader_error(diagnostic));
            }
        };
        self.adopt(loaded);
        self.reloads += 1;
        Ok(())
    }

    pub(super) fn invalidate(&mut self) {
        self.snapshot = None;
        self.invalidated = true;
    }

    pub(super) fn refresh_workflow(&mut self) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let fingerprint = input::disk_fingerprint(&self.session.config, &snapshot.loaded.input);
        if snapshot.fingerprint == fingerprint {
            return;
        }
        self.session.drop_loaded();
        self.snapshot = None;
        self.reloads += 1;
    }

    pub(super) fn adopt_session_snapshot(&mut self) {
        if self.snapshot.is_none()
            && let Some(loaded) = self.session.cached_loaded()
        {
            self.adopt(loaded);
        }
    }

    pub(super) fn loader_error(diagnostic: Diagnostic) -> super::ToolErrorInfo {
        super::ToolErrorInfo {
            class: diagnostic.class,
            fields: diagnostic.fields,
            ..super::ToolErrorInfo::carried(diagnostic.code, diagnostic.message)
        }
    }

    pub(super) fn adopt(&mut self, loaded: Loaded) {
        let semantic = self.session.shared_semantic(&loaded);
        let lint_semantic = (!self.session.config.lint.rules.is_empty())
            .then(|| semantic.with_lint_config(self.session.config.lint.clone()));
        let fingerprint = input::disk_fingerprint(&self.session.config, &loaded.input);
        self.generation += 1;
        self.snapshot = Some(Snapshot {
            loaded,
            semantic,
            lint_semantic,
            fingerprint,
        });
    }

    pub(super) fn snapshot(&self) -> &Snapshot {
        self.snapshot
            .as_ref()
            .expect("the refresh gate guarantees a snapshot")
    }

    pub(super) fn snapshot_and_semantic(&self) -> (Loaded, Arc<SemanticService<'static>>) {
        let snapshot = self.snapshot();
        (snapshot.loaded.clone(), Arc::clone(&snapshot.semantic))
    }
}
