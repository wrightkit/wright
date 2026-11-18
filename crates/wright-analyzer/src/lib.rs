/// The release version: the git tag the build was made from (see `build.rs`).
pub const VERSION: &str = env!("WRIGHT_VERSION");

pub mod analysis;
pub mod canonical;
pub mod config;
pub mod declarative;
mod first_party;
pub mod registry;
pub mod service;
