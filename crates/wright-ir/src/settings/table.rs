//! Fixture-evidenced settings emission table (#86).
//!
//! PROVENANCE: observed from the pinned oracle 9.7.10 en-US output of the
//! oracle-success settings programs (`compile.workshop` settings section of
//! the committed snapshots pixelart/santa/broken-weapons/client-to-server,
//! plus the parabola/crosshair/inputhud oracle runs) at OverPy commit
//! `eea67ad`. This is observed-behavior data, not copied OverPy source
//! (LICENSE-BOUNDARY policy). Additions to the table (e.g. the acquired
//! candidate snapshots) are data-only.

/// A leaf key kind: how a settings leaf renders and validates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    /// A quoted string (`Description: "..."`).
    String,
    /// A boolean rendered `On`/`Off`.
    Bool,
    /// A plain number.
    Number,
    /// A number rendered with a `%` suffix (`Respawn Time Scalar: 30%`).
    Percent,
    /// A string-valued enumeration with a per-domain member map
    /// (`Enum(domain)`).
    Enum(&'static str),
    /// A list of map names (`enabled maps`).
    ListMap,
    /// A list of hero names (`enabled heroes`).
    ListHero,
}

/// One segment of an exact settings path.
#[derive(Debug, Clone, Copy)]
pub enum PathPart<'a> {
    /// A literal key.
    Part(&'a str),
    /// Any game-mode slot (assault, control, escort, hybrid, skirmish, ffa,
    /// general), rendered through [`mode_name`].
    Mode,
    /// Any team slot (allTeams), rendered through [`team_name`].
    Team,
    /// Any hero-config slot, rendered through [`hero_name`].
    Hero,
}

impl<'b> PartialEq<PathPart<'b>> for PathPart<'_> {
    fn eq(&self, other: &PathPart<'b>) -> bool {
        match (self, other) {
            (PathPart::Part(left), PathPart::Part(right)) => left == right,
            (PathPart::Mode, PathPart::Mode) => true,
            (PathPart::Team, PathPart::Team) => true,
            (PathPart::Hero, PathPart::Hero) => true,
            _ => false,
        }
    }
}

impl Eq for PathPart<'_> {}

/// One table entry: an exact key path, its workshop name, and its kind.
#[derive(Debug, Clone, Copy)]
pub struct TableEntry {
    pub path: &'static [PathPart<'static>],
    pub workshop_name: &'static str,
    pub kind: KeyKind,
}

macro_rules! entry {
    ($path:expr, $name:expr, $kind:expr) => {
        TableEntry {
            path: &$path,
            workshop_name: $name,
            kind: $kind,
        }
    };
}

/// The fixture-evidenced settings surface.
///
/// Slot sets (evidenced): modes {assault, control, escort, hybrid, skirmish,
/// ffa} (per-key subsets), teams {allTeams}, heroes {mei} config groups +
/// the 10 ListHero names. `enabled: true` is not evidenced; it renders with
/// no prefix. Keys outside this table (e.g. team1Slots, scoreToWin,
/// gamemodeStartTrigger, spawnHealthPacks, healthPackRespawnTime%,
/// abilityCooldown%, healingReceived%, primaryFireKb%, enableSpawningWithUlt,
/// resetPlayersAfterGoalScored, scoreLeadToWin, gameLengthInSec,
/// heroes.<team>.general, roleLimit under general) are `settings-unknown-key`
/// at validation (only evidenced in oracle-failing programs; corpus-bounded).
pub static ENTRIES: &[TableEntry] = &[
    // main
    entry!(
        [PathPart::Part("main"), PathPart::Part("description")],
        "Description",
        KeyKind::String
    ),
    entry!(
        [PathPart::Part("main"), PathPart::Part("modeName")],
        "Mode Name",
        KeyKind::String
    ),
    // lobby
    entry!(
        [PathPart::Part("lobby"), PathPart::Part("ffaSlots")],
        "Max FFA Players",
        KeyKind::Number
    ),
    // gamemodes.<mode>
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Mode,
            PathPart::Part("enabled")
        ],
        "enabled",
        KeyKind::Bool
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Mode,
            PathPart::Part("enabledMaps")
        ],
        "enabled maps",
        KeyKind::ListMap
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Mode,
            PathPart::Part("roleLimit")
        ],
        "Limit Roles",
        KeyKind::Enum("roleLimit")
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Mode,
            PathPart::Part("enableCompetitiveRules")
        ],
        "Competitive Rules",
        KeyKind::Bool
    ),
    // gamemodes.general (the Mode slot matches `general` via the mode map)
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Mode,
            PathPart::Part("heroLimit")
        ],
        "Hero Limit",
        KeyKind::Enum("heroLimit")
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Mode,
            PathPart::Part("respawnTime%")
        ],
        "Respawn Time Scalar",
        KeyKind::Percent
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Mode,
            PathPart::Part("enableHeroSwitching")
        ],
        "Allow Hero Switching",
        KeyKind::Bool
    ),
    entry!(
        [
            PathPart::Part("gamemodes"),
            PathPart::Mode,
            PathPart::Part("enableRandomHeroes")
        ],
        "Respawn As Random Hero",
        KeyKind::Bool
    ),
    // heroes.<team>
    entry!(
        [
            PathPart::Part("heroes"),
            PathPart::Team,
            PathPart::Part("enabledHeroes")
        ],
        "enabled heroes",
        KeyKind::ListHero
    ),
    entry!(
        [
            PathPart::Part("heroes"),
            PathPart::Team,
            PathPart::Part("disabledHeroes")
        ],
        "disabled heroes",
        KeyKind::ListHero
    ),
    // heroes.<team>.<hero> config groups
    entry!(
        [
            PathPart::Part("heroes"),
            PathPart::Team,
            PathPart::Hero,
            PathPart::Part("enablePrimaryFire")
        ],
        "Primary Fire",
        KeyKind::Bool
    ),
    entry!(
        [
            PathPart::Part("heroes"),
            PathPart::Team,
            PathPart::Hero,
            PathPart::Part("enableSecondaryFire")
        ],
        "Secondary Fire",
        KeyKind::Bool
    ),
    entry!(
        [
            PathPart::Part("heroes"),
            PathPart::Team,
            PathPart::Hero,
            PathPart::Part("enableAbility1")
        ],
        "Cryo-Freeze",
        KeyKind::Bool
    ),
    entry!(
        [
            PathPart::Part("heroes"),
            PathPart::Team,
            PathPart::Hero,
            PathPart::Part("enableAbility2")
        ],
        "Ice Wall",
        KeyKind::Bool
    ),
    entry!(
        [
            PathPart::Part("heroes"),
            PathPart::Team,
            PathPart::Hero,
            PathPart::Part("health%")
        ],
        "Health",
        KeyKind::Percent
    ),
    entry!(
        [
            PathPart::Part("heroes"),
            PathPart::Team,
            PathPart::Hero,
            PathPart::Part("passiveUltGen%")
        ],
        "Ultimate Generation - Passive Blizzard",
        KeyKind::Percent
    ),
    entry!(
        [
            PathPart::Part("heroes"),
            PathPart::Team,
            PathPart::Hero,
            PathPart::Part("combatUltGen%")
        ],
        "Ultimate Generation - Combat Blizzard",
        KeyKind::Percent
    ),
];

/// A slot name mapping (key -> localized workshop name).
#[derive(Debug, Clone, Copy)]
pub struct NameMap {
    pub key: &'static str,
    pub name: &'static str,
}

/// Game-mode names (evidenced: assault, control, escort, hybrid, skirmish,
/// ffa, general).
pub static MODE_NAMES: &[NameMap] = &[
    NameMap {
        key: "assault",
        name: "Assault",
    },
    NameMap {
        key: "control",
        name: "Control",
    },
    NameMap {
        key: "escort",
        name: "Escort",
    },
    NameMap {
        key: "hybrid",
        name: "Hybrid",
    },
    NameMap {
        key: "skirmish",
        name: "Skirmish",
    },
    NameMap {
        key: "ffa",
        name: "Deathmatch",
    },
    NameMap {
        key: "general",
        name: "General",
    },
];

/// Map names inside `enabledMaps` lists.
pub static MAP_NAMES: &[NameMap] = &[
    NameMap {
        key: "workshopIsland",
        name: "Workshop Island",
    },
    NameMap {
        key: "kingsRowWinter",
        name: "King's Row Winter",
    },
];

/// Hero names inside hero lists and hero-config groups.
pub static HERO_NAMES: &[NameMap] = &[
    NameMap {
        key: "ashe",
        name: "Ashe",
    },
    NameMap {
        key: "bastion",
        name: "Bastion",
    },
    NameMap {
        key: "dva",
        name: "D.Va",
    },
    NameMap {
        key: "doomfist",
        name: "Doomfist",
    },
    NameMap {
        key: "echo",
        name: "Echo",
    },
    NameMap {
        key: "moira",
        name: "Moira",
    },
    NameMap {
        key: "reinhardt",
        name: "Reinhardt",
    },
    NameMap {
        key: "hammond",
        name: "Wrecking Ball",
    },
    NameMap {
        key: "zenyatta",
        name: "Zenyatta",
    },
    NameMap {
        key: "mei",
        name: "Mei",
    },
];

/// Team names inside `heroes` (evidenced: allTeams).
pub static TEAM_NAMES: &[NameMap] = &[NameMap {
    key: "allTeams",
    name: "General",
}];

/// An enum domain member (domain -> localized workshop name).
#[derive(Debug, Clone, Copy)]
pub struct EnumMember {
    pub domain: &'static str,
    pub member: &'static str,
    pub name: &'static str,
}

/// Enum member names per domain. `roleLimit` "off" is evidenced in the
/// acquired skirmish_elim snapshot (data-only, lands with the candidates).
pub static ENUM_MEMBERS: &[EnumMember] = &[
    EnumMember {
        domain: "roleLimit",
        member: "2OfEachRolePerTeam",
        name: "2 Of Each Role Per Team",
    },
    EnumMember {
        domain: "roleLimit",
        member: "off",
        name: "Off",
    },
    EnumMember {
        domain: "heroLimit",
        member: "off",
        name: "Off",
    },
];

/// Look up a settings leaf entry by its exact path.
pub fn lookup(path: &[PathPart<'_>]) -> Option<&'static TableEntry> {
    ENTRIES.iter().find(|entry| {
        entry.path.len() == path.len() && entry.path.iter().zip(path.iter()).all(|(a, b)| a == b)
    })
}

fn name_in(maps: &[NameMap], key: &str) -> Option<&'static str> {
    maps.iter().find(|m| m.key == key).map(|m| m.name)
}

/// The localized name of a game mode.
pub fn mode_name(key: &str) -> Option<&'static str> {
    name_in(MODE_NAMES, key)
}

/// The localized name of a map.
pub fn map_name(key: &str) -> Option<&'static str> {
    name_in(MAP_NAMES, key)
}

/// The localized name of a hero.
pub fn hero_name(key: &str) -> Option<&'static str> {
    name_in(HERO_NAMES, key)
}

/// The localized name of a team.
pub fn team_name(key: &str) -> Option<&'static str> {
    name_in(TEAM_NAMES, key)
}

/// The localized name of an enum member in a domain.
pub fn enum_name(domain: &str, member: &str) -> Option<&'static str> {
    ENUM_MEMBERS
        .iter()
        .find(|m| m.domain == domain && m.member == member)
        .map(|m| m.name)
}

/// A human-readable rendering of a path (diagnostics).
pub fn path_string(path: &[PathPart<'_>]) -> String {
    path.iter()
        .map(|part| match part {
            PathPart::Part(name) => (*name).to_string(),
            PathPart::Mode => "<mode>".to_string(),
            PathPart::Team => "<team>".to_string(),
            PathPart::Hero => "<hero>".to_string(),
        })
        .collect::<Vec<_>>()
        .join(".")
}
