//! The built-in catalog load. A provider-backed session does not read the
//! catalog until the provider answers, so the load runs on a background
//! thread while the provider connects and compiles (#1102). Every other
//! session loads it at construction, because its parser needs it first.

use super::{CompilerSession, registry};
use crate::config::{SessionConfig, SourceKind};
use crate::diag::{Diagnostic, Stage};
use crate::source_provider::SourceBackend;
use crate::trace::Trace;
use std::sync::Arc;

pub(super) struct CatalogLoad {
    #[cfg(not(target_arch = "wasm32"))]
    pending: Option<std::thread::JoinHandle<Result<(), Diagnostic>>>,
    /// The background load's failure, kept so every later workflow reports it.
    #[cfg(not(target_arch = "wasm32"))]
    failed: Option<Diagnostic>,
}

pub(super) fn load_builtin_catalog() -> Result<(), String> {
    workshop_rs::catalog::Catalog::builtin_ref()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn load_in_span(trace: &Trace, loader: fn() -> Result<(), String>) -> Result<(), Diagnostic> {
    let span = trace.parallel_span("catalog.load");
    run(&span, loader)
}

fn run(span: &crate::trace::Span, loader: fn() -> Result<(), String>) -> Result<(), Diagnostic> {
    loader().map_err(|error| {
        span.fail();
        Diagnostic::error(
            "catalog-error",
            Stage::Internal,
            format!("cannot load the built-in Workshop catalog: {error}"),
        )
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn is_provider_backed(config: &SessionConfig) -> bool {
    match config.source_backend {
        SourceBackend::Provider => true,
        SourceBackend::Auto => config.kind == SourceKind::Opy,
        SourceBackend::Native => false,
    }
}

impl CatalogLoad {
    pub(super) fn start(
        config: &SessionConfig,
        trace: &Trace,
        loader: fn() -> Result<(), String>,
    ) -> Result<CatalogLoad, Diagnostic> {
        #[cfg(not(target_arch = "wasm32"))]
        if is_provider_backed(config) {
            // The span opens here so it nests under the caller's span, not
            // under whatever span the main thread has open when the thread
            // starts.
            let span = trace.parallel_span("catalog.load");
            let pending = std::thread::spawn(move || run(&span, loader));
            return Ok(CatalogLoad {
                pending: Some(pending),
                failed: None,
            });
        }
        #[cfg(target_arch = "wasm32")]
        let _ = config;
        load_in_span(trace, loader)?;
        Ok(CatalogLoad {
            #[cfg(not(target_arch = "wasm32"))]
            pending: None,
            #[cfg(not(target_arch = "wasm32"))]
            failed: None,
        })
    }

    /// Wait for the background load, if any, and report its failure.
    pub(super) fn finish(&mut self) -> Result<(), Diagnostic> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(pending) = self.pending.take() {
                self.failed = pending
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
                    .err();
            }
            self.failed.clone().map_or(Ok(()), Err)
        }
        #[cfg(target_arch = "wasm32")]
        Ok(())
    }
}

impl CompilerSession {
    /// `loader` is the catalog load; tests substitute a failing one.
    pub(crate) fn with_catalog_loader(
        config: SessionConfig,
        trace: Trace,
        loader: fn() -> Result<(), String>,
    ) -> Result<CompilerSession, Diagnostic> {
        let catalog_load = CatalogLoad::start(&config, &trace, loader)?;
        let lint_registry = trace.in_span("lint.compose", || registry::compose(&config))?;
        Ok(CompilerSession {
            fixed_config: config.clone(),
            config,
            catalog_load,
            lint_registry: Arc::new(lint_registry),
            loaded: None,
            provider_compiled: false,
            semantic: None,
            semantic_builds: 0,
            diagnostics: Vec::new(),
            progress_observer: None,
            source_provider: None,
            single_run: false,
            trace,
        })
    }

    /// The built-in catalog. A provider-backed session loads it in the
    /// background; every workflow awaits that load (`CatalogLoad::finish`)
    /// before it reads the catalog, so this never observes a failed load.
    pub(crate) fn catalog(&self) -> &'static workshop_rs::catalog::Catalog {
        workshop_rs::catalog::Catalog::builtin_ref().expect("the catalog load was awaited")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::InputSpec;
    use crate::session::CompilerSession;
    use crate::source_provider::{
        SourceCompilation, SourceLanguage, SourceProvider, SourceProviderError, SourceTarget,
    };

    struct Provider(Result<(), ()>);

    impl SourceProvider for Provider {
        fn language(&self) -> SourceLanguage {
            SourceLanguage::Opy
        }

        fn compile(
            &mut self,
            _target: &SourceTarget,
        ) -> Result<SourceCompilation, SourceProviderError> {
            match self.0 {
                Ok(()) => Ok(SourceCompilation::success(String::new())),
                Err(()) => Err(SourceProviderError::Unsupported {
                    message: "provider failed".to_string(),
                }),
            }
        }
    }

    fn failing_catalog() -> Result<(), String> {
        Err("catalog failed".to_string())
    }

    fn session(
        provider: Provider,
        loader: fn() -> Result<(), String>,
        backend: SourceBackend,
    ) -> Result<CompilerSession, Diagnostic> {
        let config = SessionConfig {
            // Any existing file resolves; the injected provider ignores its content.
            input: InputSpec::Path(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml").into()),
            kind: SourceKind::Opy,
            source_backend: backend,
            ..SessionConfig::default()
        };
        let mut session = CompilerSession::with_catalog_loader(config, Trace::disabled(), loader)?;
        session.source_provider = Some(super::super::provider::ProviderSlot::injected(Box::new(
            provider,
        )));
        Ok(session)
    }

    fn check_code(session: &mut CompilerSession) -> String {
        session
            .load_with_operation(crate::session::ProviderOperation::Check)
            .err()
            .expect("the load is refused")
            .code
    }

    #[test]
    fn a_catalog_failure_outranks_a_provider_failure_and_a_provider_success() {
        for provider in [Provider(Err(())), Provider(Ok(()))] {
            let mut session = session(provider, failing_catalog, SourceBackend::Provider).unwrap();
            assert_eq!(check_code(&mut session), "catalog-error");
            // The failure is kept for later workflows.
            assert_eq!(check_code(&mut session), "catalog-error");
        }
    }

    #[test]
    fn a_native_session_refuses_a_catalog_failure_at_construction() {
        let error = session(Provider(Ok(())), failing_catalog, SourceBackend::Native)
            .err()
            .expect("construction fails");
        assert_eq!(error.code, "catalog-error");
    }

    #[test]
    fn a_provider_failure_is_unchanged_when_the_catalog_loads() {
        let mut session = session(
            Provider(Err(())),
            load_builtin_catalog,
            SourceBackend::Provider,
        )
        .unwrap();
        assert_eq!(check_code(&mut session), "source-provider-unsupported");
    }
}
