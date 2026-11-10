//! The one LPP connect path every Wright provider client shares (#1004):
//! resolve the provider for the language id, spawn it, initialize at the
//! preferred protocol version, and — on `protocol-version-mismatch` —
//! shut down, respawn, and retry once at the highest mutually supported
//! version above the caller's floor (LPP §19.2).
//!
//! Provider lookup is the session's registry plus the `opy` id's
//! configured resolver — there is no `installed()` fallback (the 2026-10-27
//! owner decision on #1004). Failures surface as `ProviderError`; callers
//! map them through the shared `provider-refusal-<code>` classification
//! (`crate::source_provider::provider_failure_code`).

use std::sync::Arc;

use crate::config::SessionConfig;
use crate::input::InputTarget;
use crate::opy_provider;
use crate::trace::{Fact, Trace};
use wright_lpp::LanguageProvider;
use wright_lpp::client::{ExchangeObserver, ExchangeRecord};

/// The lowest LPP protocol version a connect may negotiate. `Entry`
/// requests need 1.1 project loading; `Directory` targets need the 1.3
/// `sourceIdentity` contract they load under (#638). Document-supplied
/// requests take `None` — the base 1.0 contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectFloor {
    Entry,
    Directory,
}

impl From<InputTarget> for ConnectFloor {
    fn from(target: InputTarget) -> ConnectFloor {
        match target {
            InputTarget::Directory => ConnectFloor::Directory,
            _ => ConnectFloor::Entry,
        }
    }
}

/// The result of one [`connect_provider`] call: the spawned, initialized
/// provider session and the negotiated `lpp/initialize` answer.
pub struct ConnectedProvider {
    pub provider: Box<dyn wright_lpp::LanguageProvider>,
    pub initialize: wright_lpp::InitializeResult,
}

/// Resolve, spawn, and initialize an LPP provider session for
/// `language_id` — the single connect path every Wright LPP client uses
/// (#1004).
///
/// `preferred_version` is the client's first offer — the highest protocol
/// version its requests need. On `protocol-version-mismatch` the provider
/// is shut down, respawned, and re-initialized once at the highest version
/// both sides support that still clears `floor`; when no such version
/// exists the typed `ProtocolVersionMismatch` error answers.
///
/// A `None` floor keeps the base `1.0` contract (the language service's
/// document-supplied requests).
///
/// The connect runs inside a `provider.connect` span; each spawn is a
/// `provider.spawn` span and each exchange with the provider is an
/// `lpp.request` span (#1079).
pub fn connect_provider(
    config: &SessionConfig,
    language_id: &str,
    client: &wright_lpp::ClientInfo,
    preferred_version: &str,
    floor: Option<ConnectFloor>,
    trace: &Trace,
) -> Result<ConnectedProvider, wright_lpp::ProviderError> {
    trace.in_span("provider.connect", || {
        let providers = resolved_registry(config, language_id)?;
        let observer = exchange_observer(trace);
        let spawn = || {
            let mut provider = trace.in_span("provider.spawn", || providers.spawn(language_id))?;
            if let Some(observer) = &observer {
                provider.set_exchange_observer(Arc::clone(observer));
            }
            Ok::<_, wright_lpp::ProviderError>(provider)
        };
        let mut provider = spawn()?;
        let mut initialize = provider.initialize_version(preferred_version, Some(client));
        if initialize
            .as_ref()
            .is_err_and(|error| error.code() == "protocol-version-mismatch")
        {
            // LPP §19.2: retry at the highest mutually supported version the
            // provider advertised, honoring the caller's floor.
            let supported = initialize
                .as_ref()
                .err()
                .map(|error| error.supported_protocol_versions())
                .unwrap_or_default();
            // The old provider never initialized, so `shutdown` would be
            // rejected; reassigning drops it, and `Drop` kills the process.
            provider = spawn()?;
            initialize = match mutual_protocol_version(&supported, floor) {
                Some(version) => provider.initialize_version(version, Some(client)),
                None => Err(wright_lpp::ProviderError::ProtocolVersionMismatch {
                    supported,
                    message: format!(
                        "no mutually supported LPP protocol version serves {language_id}"
                    ),
                }),
            };
        }
        let initialize = initialize?;
        if let Some(path) = providers.command(language_id) {
            trace.set_fact("providerPath", path.display().to_string());
        }
        trace.set_fact("providerId", language_id);
        trace.set_fact("providerVersion", initialize.server_info.version.clone());
        trace.set_fact("lppVersion", initialize.protocol_version.clone());
        Ok(ConnectedProvider {
            provider: Box::new(provider),
            initialize,
        })
    })
}

/// The transport observer that turns each LPP exchange into an
/// `lpp.request` span, or `None` when the trace is disabled.
fn exchange_observer(trace: &Trace) -> Option<Arc<dyn ExchangeObserver>> {
    trace
        .is_enabled()
        .then(|| Arc::new(ExchangeTrace(trace.clone())) as Arc<dyn ExchangeObserver>)
}

struct ExchangeTrace(Trace);

impl ExchangeObserver for ExchangeTrace {
    fn exchanged(&self, record: &ExchangeRecord<'_>) {
        let mut facts = vec![
            ("method", Fact::from(record.method)),
            ("requestBytes", Fact::from(record.request_bytes)),
            ("responseBytes", Fact::from(record.response_bytes)),
        ];
        if let Some(error) = record.error {
            facts.push(("error", Fact::from(error)));
        }
        self.0
            .record("lpp.request", record.started, record.finished, facts);
    }
}

/// The registry to spawn from: the session's configured providers, with
/// the `opy` id resolved through its configured resolver when no explicit
/// registration wins (owner decision: every client resolves; no
/// `installed()` fallback). Any other unconfigured id is `NotConfigured`.
pub(crate) fn resolved_registry(
    config: &SessionConfig,
    language_id: &str,
) -> Result<wright_lpp::ProviderRegistry, wright_lpp::ProviderError> {
    if language_id != opy_provider::OPY_LANGUAGE_ID || config.providers.contains(language_id) {
        return Ok(config.providers.clone());
    }
    let resolved = config.opy_provider.resolve()?;
    let mut providers = config.providers.clone();
    providers
        .register(wright_lpp::ProviderConfig::new(
            opy_provider::OPY_LANGUAGE_ID,
            resolved.executable,
            Vec::new(),
        ))
        .expect("a registry without the 'opy' id accepts its registration");
    Ok(providers)
}

/// The highest client-supported protocol version the provider also
/// supports, at or above `floor`'s minimum (LPP §19.2).
fn mutual_protocol_version(
    supported: &[String],
    floor: Option<ConnectFloor>,
) -> Option<&'static str> {
    let minimum = match floor {
        Some(ConnectFloor::Directory) => wright_lpp::LPP_SOURCE_IDENTITY_PROTOCOL_VERSION,
        Some(ConnectFloor::Entry) => wright_lpp::LPP_PROJECT_LOADING_PROTOCOL_VERSION,
        None => wright_lpp::LPP_PROTOCOL_VERSION,
    };
    let minimum = protocol_version_tuple(minimum)?;
    wright_lpp::LPP_SUPPORTED_PROTOCOL_VERSIONS
        .iter()
        .copied()
        .filter_map(|version| protocol_version_tuple(version).map(|tuple| (tuple, version)))
        .filter(|(tuple, _)| *tuple >= minimum)
        .filter(|(_, version)| supported.iter().any(|s| s == version))
        .max_by_key(|(tuple, _)| *tuple)
        .map(|(_, version)| version)
}

fn protocol_version_tuple(version: &str) -> Option<(u32, u32)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutual_protocol_version_honors_the_target_floor() {
        let supported =
            |versions: &[&str]| versions.iter().map(|v| v.to_string()).collect::<Vec<_>>();

        assert_eq!(
            mutual_protocol_version(
                &supported(&["1.0", "1.1", "1.3"]),
                Some(ConnectFloor::Entry)
            ),
            Some("1.3")
        );
        assert_eq!(
            mutual_protocol_version(&supported(&["1.1", "1.4"]), Some(ConnectFloor::Entry)),
            Some("1.4")
        );

        assert_eq!(
            mutual_protocol_version(&supported(&["1.0"]), Some(ConnectFloor::Entry)),
            None
        );
        assert_eq!(
            mutual_protocol_version(&supported(&["1.0", "1.1"]), Some(ConnectFloor::Entry)),
            Some("1.1")
        );

        assert_eq!(
            mutual_protocol_version(&supported(&["1.1", "1.2"]), Some(ConnectFloor::Directory)),
            None
        );
        assert_eq!(
            mutual_protocol_version(&supported(&["1.2", "1.3"]), Some(ConnectFloor::Directory)),
            Some("1.3")
        );

        assert_eq!(
            mutual_protocol_version(&supported(&["1.6"]), Some(ConnectFloor::Entry)),
            None
        );
        assert_eq!(
            mutual_protocol_version(&supported(&["1.5", "1.7"]), Some(ConnectFloor::Entry)),
            Some("1.7")
        );
        assert_eq!(
            mutual_protocol_version(&supported(&["1.0"]), None),
            Some("1.0")
        );
    }

    #[test]
    fn failed_exchange_records_its_error_code() {
        let trace = Trace::enabled();
        let started = std::time::Instant::now();
        ExchangeTrace(trace.clone()).exchanged(&ExchangeRecord {
            method: "lpp/compile",
            started,
            finished: std::time::Instant::now(),
            request_bytes: 10,
            response_bytes: 0,
            error: Some("provider-exited"),
        });
        let report = trace.report().expect("enabled trace reports");
        let span = &report.spans[0];
        assert_eq!(span.name, "lpp.request");
        assert!(
            span.facts
                .contains(&("error", Fact::Text("provider-exited".into())))
        );
    }
}
