use wright_driver::opy_provider::OPY_LANGUAGE_ID;
use wright_lpp::LanguageProvider;

use super::LanguageService;

pub(super) enum OpyProviderState {
    Pending,
    Ready(Box<dyn LanguageProvider>),
    // A connect failure latches until the service restarts; a `Ready`
    // provider that dies returns to `Pending` (#1057). The
    // `(code, message)` a connect failure reports — provider error
    // classified, or a session-construction diagnostic verbatim (#1004).
    Unavailable(String, String),
}

impl LanguageService {
    /// The connected OPY provider, or the classified `(code, message)`
    /// its connect failure latched. Connects on first use and after a death.
    pub(super) fn opy_provider(&mut self) -> Result<&mut dyn LanguageProvider, (String, String)> {
        if matches!(self.opy, OpyProviderState::Pending) {
            self.opy = match self.connect_opy_provider() {
                Ok(provider) => OpyProviderState::Ready(provider),
                Err(failure) => OpyProviderState::Unavailable(failure.0, failure.1),
            };
        }
        match &mut self.opy {
            OpyProviderState::Ready(provider) => Ok(provider.as_mut()),
            OpyProviderState::Unavailable(code, message) => Err((code.clone(), message.clone())),
            OpyProviderState::Pending => unreachable!("resolved above"),
        }
    }

    /// A `Ready` provider whose request failed with a death error is dropped
    /// (which kills its process) and the state returns to `Pending`, so the
    /// next request reconnects. Other failures keep the provider (#1057).
    pub(super) fn retire_dead_provider(&mut self, error: &wright_lpp::ProviderError) {
        use wright_lpp::ProviderError;
        if matches!(
            error,
            ProviderError::Exited { .. } | ProviderError::Io { .. } | ProviderError::Timeout { .. }
        ) && matches!(self.opy, OpyProviderState::Ready(_))
        {
            self.opy = OpyProviderState::Pending;
        }
    }

    /// The OPY provider session, connected through the driver's shared
    /// connect path (#1004). The `(code, message)` failure the surface
    /// reports is a provider error classified through
    /// `provider_failure_code`, or a session-construction diagnostic
    /// carried verbatim.
    fn connect_opy_provider(&mut self) -> Result<Box<dyn LanguageProvider>, (String, String)> {
        let client = wright_lpp::ClientInfo {
            name: wright_lpp::LPP_CLIENT_NAME.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        };
        self.session()
            .map_err(|d| (d.code.clone(), d.message.clone()))?
            .connect_provider(
                OPY_LANGUAGE_ID,
                &client,
                wright_lpp::LPP_PROTOCOL_VERSION,
                None,
            )
            .map(|connected| connected.provider)
            .map_err(|e| {
                (
                    wright_driver::source_provider::provider_failure_code(&e),
                    e.to_string(),
                )
            })
    }
}
