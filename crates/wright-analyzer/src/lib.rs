/// The release version. The release workflow sets `WRIGHT_VERSION` to the git
/// tag; any other build reports the package version.
pub const VERSION: &str = match option_env!("WRIGHT_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

pub mod analysis;
pub mod canonical;
pub mod config;
pub mod declarative;
mod first_party;
pub mod registry;
pub mod service;
