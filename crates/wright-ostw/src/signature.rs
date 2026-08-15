//! Wright-owned OSTW builtin signature data (#118).
//!
//! This module owns the exact exercised Workshop action/value/enum surface of
//! the pinned protect-ban entry-point reachable graph, resolved through
//! Wright-authored data with pinned-reference provenance. It mirrors the OPY
//! semantic-manifest pattern (`wright-opy/src/manifest`): the canonical id is
//! the OSTW source name, and the en-US Workshop spelling is recorded for
//! downstream emission (#119). No OSTW `Elements.json` or upstream compiler
//! table is copied; every entry is exercised by the reachable corpus or a
//! committed pinned-reference probe under `compatibility/ostw/probes/`.
//!
//! Param order for the named-argument calls comes from the reference's
//! canonical emitted argument order (probes P6/P6b) or, where the corpus
//! calls a function positionally, the source order is preserved.

/// Whether a builtin is a Workshop action or a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinKind {
    Action,
    Value,
}

/// One exercised Workshop builtin.
#[derive(Debug, Clone, Copy)]
pub struct Builtin {
    pub kind: BuiltinKind,
    /// Canonical parameter names, in emitted order (used for named-argument
    /// binding). Empty for positional passthrough.
    pub params: &'static [&'static str],
    /// The en-US Workshop spelling (pinned-reference probe evidence).
    pub spelling: &'static str,
}

/// Resolve an exercised Workshop builtin by its OSTW source name.
pub fn builtin(name: &str) -> Option<&'static Builtin> {
    BUILTINS.iter().find(|(id, _)| *id == name).map(|(_, b)| b)
}

/// An exercised builtin enum domain and its members.
#[derive(Debug, Clone, Copy)]
pub struct EnumDomain {
    /// The en-US Workshop spelling of the domain.
    pub spelling: &'static str,
    pub members: &'static [&'static str],
}

/// Resolve an exercised builtin enum domain by its OSTW source name.
pub fn enum_domain(name: &str) -> Option<&'static EnumDomain> {
    ENUM_DOMAINS
        .iter()
        .find(|(id, _)| *id == name)
        .map(|(_, domain)| domain)
}

/// `name -> Builtin`. Provenance: protect-ban reachable corpus usage and the
/// pinned-reference probes P6/P6b (canonical emitted argument order).
const BUILTINS: &[(&str, Builtin)] = &[
    // --- settings / combo values ------------------------------------------
    (
        "WorkshopSettingInteger",
        Builtin {
            kind: BuiltinKind::Value,
            params: &[
                "Category",
                "Name",
                "Default",
                "MinValue",
                "MaxValue",
                "SortOrder",
            ],
            spelling: "Workshop Setting Integer",
        },
    ),
    (
        "WorkshopSettingToggle",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Category", "Name", "Default", "SortOrder"],
            spelling: "Workshop Setting Toggle",
        },
    ),
    (
        "WorkshopSettingCombo",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Category", "Name", "Default", "Options", "SortOrder"],
            spelling: "Workshop Setting Combo",
        },
    ),
    // --- player/hero/team collections --------------------------------------
    (
        "AllPlayers",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Team"],
            spelling: "All Players",
        },
    ),
    (
        "AllHeroes",
        Builtin {
            kind: BuiltinKind::Value,
            params: &[],
            spelling: "All Heroes",
        },
    ),
    (
        "AllTankHeroes",
        Builtin {
            kind: BuiltinKind::Value,
            params: &[],
            spelling: "All Tank Heroes",
        },
    ),
    (
        "AllDamageHeroes",
        Builtin {
            kind: BuiltinKind::Value,
            params: &[],
            spelling: "All Damage Heroes",
        },
    ),
    (
        "AllSupportHeroes",
        Builtin {
            kind: BuiltinKind::Value,
            params: &[],
            spelling: "All Support Heroes",
        },
    ),
    (
        "AllowedHeroes",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Player"],
            spelling: "Allowed Heroes",
        },
    ),
    (
        "EventPlayer",
        Builtin {
            kind: BuiltinKind::Value,
            params: &[],
            spelling: "Event Player",
        },
    ),
    (
        "LocalPlayer",
        Builtin {
            kind: BuiltinKind::Value,
            params: &[],
            spelling: "Local Player",
        },
    ),
    (
        "TeamOf",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Player"],
            spelling: "Team Of",
        },
    ),
    (
        "OppositeTeamOf",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Team"],
            spelling: "Opposite Team Of",
        },
    ),
    (
        "NumberOfPlayers",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Team"],
            spelling: "Number Of Players",
        },
    ),
    // --- arrays -------------------------------------------------------------
    (
        "ArrayContains",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Array", "Value"],
            spelling: "Array Contains",
        },
    ),
    (
        "ArrayElement",
        Builtin {
            kind: BuiltinKind::Value,
            params: &[],
            spelling: "Array Element",
        },
    ),
    (
        "CurrentArrayIndex",
        Builtin {
            kind: BuiltinKind::Value,
            params: &[],
            spelling: "Current Array Index",
        },
    ),
    (
        "CountOf",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Array"],
            spelling: "Count Of",
        },
    ),
    (
        "IndexOfArrayValue",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Array", "Value"],
            spelling: "Index Of Array Value",
        },
    ),
    (
        "RandomValueInArray",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Array"],
            spelling: "Random Value In Array",
        },
    ),
    (
        "MappedArray",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Array", "Map"],
            spelling: "Mapped Array",
        },
    ),
    (
        "FilteredArray",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Array", "Condition"],
            spelling: "Filtered Array",
        },
    ),
    (
        "SortedArray",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Array", "Sort"],
            spelling: "Sorted Array",
        },
    ),
    (
        "LastOf",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Array"],
            spelling: "Last Of",
        },
    ),
    (
        "EmptyArray",
        Builtin {
            kind: BuiltinKind::Value,
            params: &[],
            spelling: "Empty Array",
        },
    ),
    (
        "RemoveFromArray",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Array", "Value"],
            spelling: "Remove From Array",
        },
    ),
    (
        "Append",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Array", "Value"],
            spelling: "Append",
        },
    ),
    // --- math / vector ------------------------------------------------------
    (
        "Max",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Value", "Value"],
            spelling: "Max",
        },
    ),
    (
        "Min",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Value", "Value"],
            spelling: "Min",
        },
    ),
    (
        "RoundToInteger",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Value", "Rounding"],
            spelling: "Round To Integer",
        },
    ),
    (
        "Vector",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["X", "Y", "Z"],
            spelling: "Vector",
        },
    ),
    (
        "CrossProduct",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Vector", "Vector"],
            spelling: "Cross Product",
        },
    ),
    (
        "DirectionFromAngles",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["HorizontalAngle", "VerticalAngle"],
            spelling: "Direction From Angles",
        },
    ),
    (
        "HorizontalAngleFromDirection",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Direction"],
            spelling: "Horizontal Angle From Direction",
        },
    ),
    (
        "VerticalAngleFromDirection",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Direction"],
            spelling: "Vertical Angle From Direction",
        },
    ),
    (
        "Forward",
        Builtin {
            kind: BuiltinKind::Value,
            params: &[],
            spelling: "Forward",
        },
    ),
    (
        "CustomColor",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Red", "Green", "Blue", "Alpha"],
            spelling: "Custom Color",
        },
    ),
    // --- state / query values ----------------------------------------------
    (
        "HasSpawned",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Player"],
            spelling: "Has Spawned",
        },
    ),
    (
        "IsButtonHeld",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Player", "Button"],
            spelling: "Is Button Held",
        },
    ),
    (
        "IsInSpawnRoom",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Player"],
            spelling: "Is In Spawn Room",
        },
    ),
    (
        "IsTrueForAll",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Array", "Condition"],
            spelling: "Is True For All",
        },
    ),
    (
        "IsWaitingForPlayers",
        Builtin {
            kind: BuiltinKind::Value,
            params: &[],
            spelling: "Is Waiting For Players",
        },
    ),
    (
        "CurrentMap",
        Builtin {
            kind: BuiltinKind::Value,
            params: &[],
            spelling: "Current Map",
        },
    ),
    (
        "EvaluateOnce",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Value"],
            spelling: "Evaluate Once",
        },
    ),
    (
        "UpdateEveryFrame",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Value"],
            spelling: "Update Every Frame",
        },
    ),
    (
        "LastCreatedEntity",
        Builtin {
            kind: BuiltinKind::Value,
            params: &[],
            spelling: "Last Created Entity",
        },
    ),
    (
        "LastTextID",
        Builtin {
            kind: BuiltinKind::Value,
            params: &[],
            spelling: "Last Text ID",
        },
    ),
    // --- icon / string helpers ----------------------------------------------
    (
        "HeroIconString",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Hero"],
            spelling: "Hero Icon String",
        },
    ),
    (
        "AbilityIconString",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Hero", "Button"],
            spelling: "Ability Icon String",
        },
    ),
    (
        "IconString",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Icon"],
            spelling: "Icon String",
        },
    ),
    (
        "InputBindingString",
        Builtin {
            kind: BuiltinKind::Value,
            params: &["Button"],
            spelling: "Input Binding String",
        },
    ),
    // --- actions ------------------------------------------------------------
    (
        "BigMessage",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["VisibleTo", "Header"],
            spelling: "Big Message",
        },
    ),
    (
        "SmallMessage",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["VisibleTo", "Header"],
            spelling: "Small Message",
        },
    ),
    (
        "Wait",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Duration", "WaitBehavior"],
            spelling: "Wait",
        },
    ),
    (
        "WaitUntil",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Condition", "Timeout"],
            spelling: "Wait Until",
        },
    ),
    (
        "MinWait",
        Builtin {
            kind: BuiltinKind::Action,
            params: &[],
            spelling: "Wait",
        },
    ),
    (
        "Skip",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Value"],
            spelling: "Skip",
        },
    ),
    (
        "LoopIfConditionIsTrue",
        Builtin {
            kind: BuiltinKind::Action,
            params: &[],
            spelling: "Loop If Condition Is True",
        },
    ),
    (
        "AbortIf",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Condition"],
            spelling: "Abort If",
        },
    ),
    (
        "ModifyVariable",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Variable", "Operation", "Value"],
            spelling: "Modify Global Variable",
        },
    ),
    (
        "CreateEffect",
        Builtin {
            kind: BuiltinKind::Action,
            params: &[
                "VisibleTo",
                "Type",
                "Color",
                "Position",
                "Radius",
                "Reevaluation",
            ],
            spelling: "Create Effect",
        },
    ),
    (
        "CreateInWorldText",
        Builtin {
            kind: BuiltinKind::Action,
            params: &[
                "VisibleTo",
                "Header",
                "Position",
                "Scale",
                "Clipping",
                "Reevaluation",
                "TextColor",
                "Spectators",
            ],
            spelling: "Create In-World Text",
        },
    ),
    (
        "CreateProgressBarInWorldText",
        Builtin {
            kind: BuiltinKind::Action,
            params: &[
                "VisibleTo",
                "Value",
                "Text",
                "Position",
                "Scale",
                "Clipping",
                "HeaderColor",
                "TextColor",
                "Reevaluation",
                "NonteamSpectators",
            ],
            spelling: "Create Progress Bar In-World Text",
        },
    ),
    (
        "CreateHudText",
        Builtin {
            kind: BuiltinKind::Action,
            params: &[
                "VisibleTo",
                "Header",
                "Subheader",
                "Text",
                "Location",
                "SortOrder",
                "HeaderColor",
                "SubheaderColor",
                "TextColor",
                "Reevaluation",
                "Spectators",
            ],
            spelling: "Create HUD Text",
        },
    ),
    (
        "PlayEffect",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["VisibleTo", "Type", "Color", "Position", "Radius"],
            spelling: "Play Effect",
        },
    ),
    (
        "StartCamera",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Player", "EyePosition", "LookAtPosition", "Facing"],
            spelling: "Start Camera",
        },
    ),
    (
        "StopCamera",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Player"],
            spelling: "Stop Camera",
        },
    ),
    (
        "StartGameMode",
        Builtin {
            kind: BuiltinKind::Action,
            params: &[],
            spelling: "Start Game Mode",
        },
    ),
    (
        "SetInvisible",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Player", "InvisibleTo"],
            spelling: "Set Invisible",
        },
    ),
    (
        "SetGravity",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Player", "Gravity"],
            spelling: "Set Gravity",
        },
    ),
    (
        "SetAllowedHeroes",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Player", "Heroes"],
            spelling: "Set Allowed Heroes",
        },
    ),
    (
        "ForcePlayerHero",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Player", "Hero"],
            spelling: "Force Player Hero",
        },
    ),
    (
        "StopForcingHero",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Player"],
            spelling: "Stop Forcing Hero",
        },
    ),
    (
        "ForceThrottle",
        Builtin {
            kind: BuiltinKind::Action,
            params: &[
                "Player",
                "MoveSpeed",
                "InAirSpeed",
                "SpectatorSpeed",
                "GrappleBoost",
                "JumpPower",
                "MoveSpeed",
            ],
            spelling: "Force Throttle",
        },
    ),
    (
        "StopForcingThrottle",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Player"],
            spelling: "Stop Forcing Throttle",
        },
    ),
    (
        "DisableGameModeHud",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Player"],
            spelling: "Disable Game Mode HUD",
        },
    ),
    (
        "DisableGameModeInworldUI",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Player"],
            spelling: "Disable Game Mode In-World UI",
        },
    ),
    (
        "DisableHeroHud",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Player"],
            spelling: "Disable Hero HUD",
        },
    ),
    (
        "DisableScoreboard",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Player"],
            spelling: "Disable Scoreboard",
        },
    ),
    (
        "DisableInspectorRecording",
        Builtin {
            kind: BuiltinKind::Action,
            params: &[],
            spelling: "Disable Inspector Recording",
        },
    ),
    (
        "EnableGameModeHud",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Player"],
            spelling: "Enable Game Mode HUD",
        },
    ),
    (
        "EnableGameModeInworldUI",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Player"],
            spelling: "Enable Game Mode In-World UI",
        },
    ),
    (
        "EnableHeroHud",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Player"],
            spelling: "Enable Hero HUD",
        },
    ),
    (
        "EnableScoreboard",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Player"],
            spelling: "Enable Scoreboard",
        },
    ),
    (
        "EnableInspectorRecording",
        Builtin {
            kind: BuiltinKind::Action,
            params: &[],
            spelling: "Enable Inspector Recording",
        },
    ),
    (
        "DisableMovementCollisionWithEnvironment",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Player", "IncludeFloors"],
            spelling: "Disable Movement Collision With Environment",
        },
    ),
    (
        "DisableMovementCollisionWithPlayers",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Player"],
            spelling: "Disable Movement Collision With Players",
        },
    ),
    (
        "EnableMovementCollisionWithEnvironment",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Player"],
            spelling: "Enable Movement Collision With Environment",
        },
    ),
    (
        "EnableMovementCollisionWithPlayers",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Player"],
            spelling: "Enable Movement Collision With Players",
        },
    ),
    (
        "DisallowButton",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Player", "Button"],
            spelling: "Disallow Button",
        },
    ),
    (
        "AllowButton",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Player", "Button"],
            spelling: "Allow Button",
        },
    ),
    (
        "DestroyHudText",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["TextId"],
            spelling: "Destroy HUD Text",
        },
    ),
    (
        "DestroyInWorldText",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["TextId"],
            spelling: "Destroy In-World Text",
        },
    ),
    (
        "DestroyEffect",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["EffectId"],
            spelling: "Destroy Effect",
        },
    ),
    (
        "DestroyAllProgressBarHudText",
        Builtin {
            kind: BuiltinKind::Action,
            params: &[],
            spelling: "Destroy All Progress Bar HUD Text",
        },
    ),
    (
        "DestroyAllProgressBarInWorldText",
        Builtin {
            kind: BuiltinKind::Action,
            params: &[],
            spelling: "Destroy All Progress Bar In-World Text",
        },
    ),
    (
        "DeleteAllClasses",
        Builtin {
            kind: BuiltinKind::Action,
            params: &[],
            spelling: "Delete All Classes",
        },
    ),
    (
        "StopChasingVariable",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Variable"],
            spelling: "Stop Chasing Variable",
        },
    ),
    (
        "ChaseVariableAtRate",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Variable", "Destination", "Rate", "Reevaluation"],
            spelling: "Chase Variable At Rate",
        },
    ),
    (
        "Teleport",
        Builtin {
            kind: BuiltinKind::Action,
            params: &["Player", "Position"],
            spelling: "Teleport",
        },
    ),
];

/// `domain -> EnumDomain`. Members from the reachable corpus usage; the en-US
/// spelling from the pinned-reference probe emissions.
const ENUM_DOMAINS: &[(&str, EnumDomain)] = &[
    (
        "Team",
        EnumDomain {
            spelling: "Team",
            members: &["All", "Team1", "Team2"],
        },
    ),
    (
        "Button",
        EnumDomain {
            spelling: "Button",
            members: &[
                "PrimaryFire",
                "SecondaryFire",
                "Ability1",
                "Ability2",
                "Ultimate",
                "Crouch",
                "Interact",
                "Jump",
                "Melee",
                "Reload",
            ],
        },
    ),
    (
        "Clipping",
        EnumDomain {
            spelling: "Clipping",
            members: &["DoNotClip", "ClipAgainstSurfaces"],
        },
    ),
    (
        "Color",
        EnumDomain {
            spelling: "Color",
            members: &[
                "White",
                "Yellow",
                "Green",
                "Purple",
                "Red",
                "Blue",
                "Aqua",
                "Orange",
                "SkyBlue",
                "Turquoise",
                "LimeGreen",
                "Gray",
                "Violet",
                "Rose",
                "Black",
                "Team1",
                "Team2",
            ],
        },
    ),
    (
        "Effect",
        EnumDomain {
            spelling: "Effect",
            members: &["Orb"],
        },
    ),
    (
        "EffectRev",
        EnumDomain {
            spelling: "Effect Reevaluation",
            members: &["VisibleToPositionAndRadius"],
        },
    ),
    (
        "Hero",
        EnumDomain {
            spelling: "Hero",
            members: &[
                "Dva",
                "Orisa",
                "Reinhardt",
                "Roadhog",
                "Sigma",
                "WreckingBall",
                "Winston",
                "Zarya",
                "Ashe",
                "Bastion",
                "Cassidy",
                "Doomfist",
                "Echo",
                "Genji",
                "Hanzo",
                "Junkrat",
                "Mei",
                "Pharah",
                "Reaper",
                "Soldier76",
                "Symmetra",
                "Sombra",
                "Tracer",
                "Torbjorn",
                "Widowmaker",
                "Ana",
                "Brigitte",
                "Baptiste",
                "Lucio",
                "Moira",
                "Mercy",
                "Zenyatta",
            ],
        },
    ),
    (
        "HudTextRev",
        EnumDomain {
            spelling: "HUD Text Reevaluation",
            members: &[
                "VisibleTo",
                "VisibleToAndString",
                "VisibleToStringAndColor",
                "VisibleToAndColor",
            ],
        },
    ),
    (
        "Icon",
        EnumDomain {
            spelling: "Icon",
            members: &["No", "QuestionMark", "Skull", "Checkmark", "RingThin"],
        },
    ),
    (
        "InvisibleTo",
        EnumDomain {
            spelling: "Invisibility",
            members: &["All", "None"],
        },
    ),
    (
        "Map",
        EnumDomain {
            spelling: "Map",
            members: &[
                "Hanamura",
                "Hanamura_Winter",
                "Horizon_Lunar_Colony",
                "Paris",
                "Temple_of_Anubis",
                "Volskaya_Industries",
                "Hanaoka",
                "Throne_of_Anubis",
                "Antarctic_Peninsula",
                "Busan",
                "Ilios",
                "Lijiang_Tower",
                "Lijiang_Tower_Lunar",
                "Nepal",
                "Oasis",
                "Samoa",
                "Circuit_Royal",
                "Dorado",
                "Havana",
                "Junkertown",
                "Rialto",
                "Route_66",
                "Shambali_Monastery",
                "Watchpoint_Gibraltar",
                "Aatlis",
                "New_Junk_City",
                "Suravasa",
                "Blizzard_World",
                "Blizzard_World_Winter",
                "Eichenwalde",
                "Eichenwalde_Halloween",
                "Hollywood",
                "Hollywood_Halloween",
                "Kings_Row",
                "Kings_Row_Winter",
                "Midtown",
                "Numbani",
                "Paraiso",
                "Colosseo",
                "Esperanca",
                "New_Queen_Street",
                "Runasapi",
            ],
        },
    ),
    (
        "InworldTextRev",
        EnumDomain {
            spelling: "In-World Text Reevaluation",
            members: &[
                "VisibleTo",
                "VisibleToAndColor",
                "VisibleToAndPosition",
                "VisibleToAndString",
                "VisibleToPositionAndColor",
                "VisibleToPositionAndString",
                "VisibleToPositionStringAndColor",
                "VisibleToStringAndColor",
                "String",
            ],
        },
    ),
    (
        "Location",
        EnumDomain {
            spelling: "Location",
            members: &["Left", "Right"],
        },
    ),
    (
        "Operation",
        EnumDomain {
            spelling: "Operation",
            members: &[
                "AppendToArray",
                "RemoveFromArrayByValue",
                "RemoveFromArrayByIndex",
            ],
        },
    ),
    (
        "PlayEffect",
        EnumDomain {
            spelling: "Play Effect",
            members: &[
                "BuffImpactSound",
                "DebuffImpactSound",
                "BuffExplosionSound",
                "ExplosionSound",
                "RingExplosionSound",
            ],
        },
    ),
    (
        "ProgressBarWorldEvaluation",
        EnumDomain {
            spelling: "Progress Bar In-World Text Reevaluation",
            members: &["VisibleToAndValues"],
        },
    ),
    (
        "RateChaseReevaluation",
        EnumDomain {
            spelling: "Rate Chase Reevaluation",
            members: &["None", "DestinationAndRate"],
        },
    ),
    (
        "Rounding",
        EnumDomain {
            spelling: "Rounding",
            members: &["Up", "Down", "Nearest"],
        },
    ),
    (
        "Spectators",
        EnumDomain {
            spelling: "Spectators",
            members: &["DefaultVisibility", "VisibleAlways", "VisibleNever"],
        },
    ),
    (
        "WaitBehavior",
        EnumDomain {
            spelling: "Wait Behavior",
            members: &["AbortWhenFalse", "IgnoreCondition"],
        },
    ),
];

/// The builtin enums whose members the reachable graph reads with `Type.Member`
/// qualified names (used by the resolver to accept `Enum` expressions).
pub fn is_known_domain(name: &str) -> bool {
    ENUM_DOMAINS.iter().any(|(id, _)| *id == name)
}
