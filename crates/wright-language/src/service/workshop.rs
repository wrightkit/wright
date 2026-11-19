//! The cached Workshop analysis for one open document (#1005): the
//! loaded `CompilerSession` and the semantic index built over it, keyed
//! by the document's `(uri, version)`. Repeated diagnostics and
//! navigation requests over an unchanged buffer reuse one build; a
//! `didChange` version bump rebuilds once on the next request. Split
//! from `service.rs` so the service stays under the context-size limit.

use wright_analyzer::canonical::SemanticIndex;
use wright_driver::{CompilerSession, InputSpec, SessionConfig, SourceKind};

use crate::document::{Document, uri_to_path};

/// The cached Workshop analysis for one open document (#1005): the loaded
/// session and the semantic index built over it, keyed by the document's
/// `(uri, version)`. Repeated diagnostics and navigation requests over an
/// unchanged buffer reuse one build; a `didChange` version bump rebuilds
/// once on the next request.
pub(super) struct WorkshopAnalysis {
    uri: String,
    version: i32,
    pub(super) session: CompilerSession,
    pub(super) index: SemanticIndex,
}

impl WorkshopAnalysis {
    fn build(
        config: &SessionConfig,
        document: &Document,
    ) -> Result<WorkshopAnalysis, wright_driver::Diagnostic> {
        let mut session = CompilerSession::new(SessionConfig {
            input: InputSpec::Text {
                text: document.text.clone(),
                path: uri_to_path(&document.uri),
            },
            kind: SourceKind::Workshop,
            root: Some(document.root.clone()),
            ..config.clone()
        })?;
        let loaded = session.load()?;
        Ok(WorkshopAnalysis {
            uri: document.uri.clone(),
            version: document.version,
            index: SemanticIndex::build(&loaded.program),
            session,
        })
    }

    fn matches(&self, document: &Document) -> bool {
        self.uri == document.uri && self.version == document.version
    }
}

#[derive(Default)]
pub(super) struct WorkshopCache {
    analysis: Option<WorkshopAnalysis>,
    pub(super) builds: usize,
}

impl WorkshopCache {
    pub(super) fn get_or_build(
        &mut self,
        config: &SessionConfig,
        document: &Document,
    ) -> Result<&mut WorkshopAnalysis, wright_driver::Diagnostic> {
        let current = self
            .analysis
            .as_ref()
            .is_some_and(|analysis| analysis.matches(document));
        if !current {
            self.analysis = Some(WorkshopAnalysis::build(config, document)?);
            self.builds += 1;
        }
        Ok(self.analysis.as_mut().expect("analysis is populated"))
    }
}
