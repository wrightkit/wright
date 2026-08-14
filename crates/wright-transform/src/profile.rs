//! Transformation profiles.

use serde::{Deserialize, Serialize};

/// The transformation policy for a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    /// No transformation: the clean reference path.
    #[default]
    Off,
    /// Evidence-backed, compatibility-safe passes (fold-constants).
    ///
    /// Source-semantic behavior (declaration initializers) is owned by the
    /// profile-independent HIR → WIR lowering, never by a profile pass
    /// (#112).
    Compat,
    /// Experimental marker; in v1 selects the same evidence-backed passes.
    Aggressive,
}

impl Profile {
    /// The canonical CLI spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Profile::Off => "off",
            Profile::Compat => "compat",
            Profile::Aggressive => "aggressive",
        }
    }

    /// Parse a CLI spelling.
    pub fn parse(name: &str) -> Option<Profile> {
        Some(match name {
            "off" => Profile::Off,
            "compat" => Profile::Compat,
            "aggressive" => Profile::Aggressive,
            _ => return None,
        })
    }
}
