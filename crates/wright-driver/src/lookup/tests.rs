use super::lookup_failure_class;
use crate::diag::ExitClass;
use wright_lpp::{LppErrorKind, ProviderError};

/// `lookup`'s provider-error exits keep the values the surface published
/// before classes were typed (#1003): the LPP `invalid-*` kinds stay
/// USAGE, and `lpp-error` plus the session-state failures stay
/// SOURCE_ERROR, while every other provider failure keeps the shared
/// classification.
#[test]
fn provider_failures_keep_their_lookup_exit_class() {
    let lpp = |kind| ProviderError::lpp(kind, serde_json::json!({}), "m");
    // (error, expected class, expected exit)
    let cases: &[(ProviderError, ExitClass, u8)] = &[
        // usage — the exits main's `invalid-` code mapping produced
        (lpp(LppErrorKind::InvalidRequest), ExitClass::Usage, 2),
        (lpp(LppErrorKind::InvalidLanguage), ExitClass::Usage, 2),
        (lpp(LppErrorKind::InvalidDocument), ExitClass::Usage, 2),
        (lpp(LppErrorKind::InvalidEntry), ExitClass::Usage, 2),
        (lpp(LppErrorKind::InvalidPosition), ExitClass::Usage, 2),
        (lpp(LppErrorKind::InvalidArtifact), ExitClass::Usage, 2),
        // source/user error — non-`invalid-` codes carried at exit 1;
        // every unknown LPP kind reports the `lpp-error` code
        (
            lpp(LppErrorKind::Unknown("invalidFutureKind".to_string())),
            ExitClass::SourceError,
            1,
        ),
        (
            lpp(LppErrorKind::Unknown("someFutureKind".to_string())),
            ExitClass::SourceError,
            1,
        ),
        (lpp(LppErrorKind::Refusal), ExitClass::SourceError, 1),
        (
            lpp(LppErrorKind::ProjectLoadFailed),
            ExitClass::SourceError,
            1,
        ),
        (
            ProviderError::NotInitialized {
                method: "lpp/lookup".to_string(),
            },
            ExitClass::SourceError,
            1,
        ),
        (ProviderError::AlreadyInitialized, ExitClass::SourceError, 1),
        (
            ProviderError::ShutDown {
                method: "lpp/lookup".to_string(),
            },
            ExitClass::SourceError,
            1,
        ),
        // unsupported
        (
            lpp(LppErrorKind::CapabilityUnavailable),
            ExitClass::Unsupported,
            3,
        ),
        // internal — the transport/session failures the contract already
        // documents at exit 4
        (
            lpp(LppErrorKind::ProtocolVersionMismatch),
            ExitClass::Internal,
            4,
        ),
        (
            ProviderError::Timeout {
                method: "lpp/lookup".to_string(),
                duration: std::time::Duration::from_secs(5),
            },
            ExitClass::Internal,
            4,
        ),
        (
            ProviderError::Malformed {
                detail: "missing id".to_string(),
            },
            ExitClass::Internal,
            4,
        ),
        (
            ProviderError::JsonRpc {
                code: -32600,
                message: "invalid request".to_string(),
            },
            ExitClass::Internal,
            4,
        ),
    ];
    for (error, class, expected) in cases {
        let actual = lookup_failure_class(error);
        assert_eq!(&actual, class, "{error:?}");
        assert_eq!(actual.exit_code(), *expected, "{error:?}");
    }
}
