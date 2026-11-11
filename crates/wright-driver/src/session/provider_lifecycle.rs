#[cfg(not(target_arch = "wasm32"))]
use super::diagnostics::provider_error_diagnostic;
use super::*;

impl CompilerSession {
    /// Consume the session in one workflow, releasing its source provider after loading.
    pub fn run_once<T>(mut self, run: impl FnOnce(&mut Self) -> T) -> T {
        self.single_run = true;
        run(&mut self)
    }

    /// Spawn the configured provider without initializing its session.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn language_provider(
        &self,
        id: &str,
    ) -> Result<Box<dyn wright_lpp::LanguageProvider>, wright_lpp::ProviderError> {
        if let Err(diagnostic) = self.verify_fixed_config() {
            return Err(wright_lpp::ProviderError::lpp(
                wright_lpp::LppErrorKind::Refusal,
                serde_json::json!({ "refusalCode": diagnostic.code }),
                diagnostic.message,
            ));
        }
        crate::connect::resolved_registry(&self.config, id)?
            .spawn(id)
            .map(|b| Box::new(b) as Box<dyn wright_lpp::LanguageProvider>)
    }

    /// Connect and negotiate a provider, retrying at the highest mutually supported version.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn connect_provider(
        &self,
        language_id: &str,
        client: &wright_lpp::ClientInfo,
        preferred_version: &str,
        floor: Option<crate::connect::ConnectFloor>,
    ) -> Result<crate::connect::ConnectedProvider, wright_lpp::ProviderError> {
        if let Err(diagnostic) = self.verify_fixed_config() {
            return Err(wright_lpp::ProviderError::lpp(
                wright_lpp::LppErrorKind::Refusal,
                serde_json::json!({ "refusalCode": diagnostic.code }),
                diagnostic.message,
            ));
        }
        crate::connect::connect_provider(
            &self.config,
            language_id,
            client,
            preferred_version,
            floor,
            &self.trace,
        )
    }

    /// Run a provider mutation flow through a fresh connection and terminate it gracefully.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn run_provider_flow(
        &self,
        language_id: &str,
        client: &wright_lpp::ClientInfo,
        flow: impl FnOnce(
            &mut dyn wright_lpp::LanguageProvider,
        ) -> crate::provider_edit::ProviderMutation,
    ) -> crate::provider_edit::ProviderMutation {
        let mut provider = match self.connect_provider(
            language_id,
            client,
            wright_lpp::LPP_PROTOCOL_VERSION,
            None,
        ) {
            Ok(connected) => connected.provider,
            Err(error) => return crate::provider_edit::provider_failure(&error),
        };
        let mutation = flow(provider.as_mut());
        let _ = provider.shutdown();
        mutation
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn convert_opy(&mut self, _loaded: &Loaded) -> Result<String, ()> {
        self.diagnostics.push(source_provider_unavailable());
        Err(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn convert_opy(&mut self, loaded: &Loaded) -> Result<String, ()> {
        let locale = Self::locale_for(loaded);
        let artifact =
            workshop_rs::emitter::emit(&loaded.program, self.catalog, &locale).map_err(|e| {
                self.diagnostics.push(Diagnostic::error(
                    "workshop-emission",
                    Stage::Emission,
                    e.to_string(),
                ));
            })?;
        let mut provider = self
            .connect_provider(
                opy_provider::OPY_LANGUAGE_ID,
                &lpp_client_info(),
                wright_lpp::LPP_PROTOCOL_VERSION,
                None,
            )
            .map_err(|e| {
                self.diagnostics
                    .push(provider_error_diagnostic(e, "initialize"));
            })?
            .provider;
        let result = provider
            .reconstruct(&wright_lpp::WorkshopArtifact {
                format: "workshop-rs/text-v1".to_string(),
                content: artifact,
            })
            .map_err(|e| {
                self.diagnostics
                    .push(provider_error_diagnostic(e, "reconstruct"));
            })?;
        let _ = provider.shutdown();
        Ok(result.source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source_provider::{SourceCompilation, SourceLanguage, SourceTarget};
    use std::sync::Mutex;

    struct RecordingProvider(Arc<Mutex<Vec<&'static str>>>);

    impl SourceProvider for RecordingProvider {
        fn language(&self) -> SourceLanguage {
            SourceLanguage::Opy
        }

        fn compile(&mut self, _: &SourceTarget) -> Result<SourceCompilation, SourceProviderError> {
            self.0.lock().unwrap().push("compile");
            Ok(SourceCompilation::success(
                "rule (\"tick\") { event { Ongoing - Global; } actions { Wait(1, Ignore Condition); } }",
            ))
        }

        fn begin_shutdown(&mut self) {
            self.0.lock().unwrap().push("shutdown");
        }
    }

    impl Drop for RecordingProvider {
        fn drop(&mut self) {
            self.0.lock().unwrap().push("drop");
        }
    }

    #[test]
    fn single_run_releases_after_loading_and_reusable_sessions_keep_the_provider() {
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path().join("main.opy");
        std::fs::write(&entry, "provider-owned source").unwrap();
        for single_run in [false, true] {
            let events = Arc::new(Mutex::new(Vec::new()));
            let mut session = CompilerSession::with_source_provider(
                SessionConfig {
                    input: InputSpec::Path(entry.clone()),
                    kind: SourceKind::Opy,
                    ..SessionConfig::default()
                },
                Box::new(RecordingProvider(Arc::clone(&events))),
            )
            .unwrap();
            if single_run {
                session.run_once(|session| {
                    assert!(session.compile().ok);
                    assert_eq!(*events.lock().unwrap(), ["compile", "shutdown"]);
                });
                assert_eq!(*events.lock().unwrap(), ["compile", "shutdown", "drop"]);
            } else {
                assert!(session.compile().ok);
                session.reload().expect("reusable provider reloads");
                assert_eq!(*events.lock().unwrap(), ["compile", "compile"]);
            }
        }
    }
}
