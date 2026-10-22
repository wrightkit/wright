//! The LPP protocol-version negotiation helpers the source-provider
//! session path shares: highest mutual version above the request's
//! floor, and the `major.minor` tuple comparison.

use crate::input::InputTarget;

/// The highest LPP version both sides implement, honoring the target's
/// negotiation floor (LPP §19.2, #638): entry requests cannot drop below
/// 1.1 project loading, and a directory target cannot drop below the 1.3
/// `sourceIdentity` contract its canonical load depends on.
pub(super) fn mutual_protocol_version(
    supported: &[String],
    target: InputTarget,
) -> Option<&'static str> {
    let floor = match target {
        InputTarget::Directory => wright_lpp::LPP_SOURCE_IDENTITY_PROTOCOL_VERSION,
        _ => wright_lpp::LPP_PROJECT_LOADING_PROTOCOL_VERSION,
    };
    let floor = protocol_version_tuple(floor)?;
    wright_lpp::LPP_SUPPORTED_PROTOCOL_VERSIONS
        .iter()
        .copied()
        .filter_map(|version| protocol_version_tuple(version).map(|tuple| (tuple, version)))
        .filter(|(tuple, _)| *tuple >= floor)
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
