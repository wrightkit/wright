//! Emitter tests (#34): byte-stable localized emission, round-trip
//! equivalence, and structured failure for unsupported/unknown output.

use std::path::{Path, PathBuf};

use wright_ir::wir;
use wright_workshop::catalog::{Catalog, Locale};
use wright_workshop::emitter;
use wright_workshop::parser;

fn oracle_path(fixture_id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compatibility/fixtures")
        .join(fixture_id)
        .join("oracle.json")
}

fn corpus_text(fixture_id: &str) -> String {
    let oracle = serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(oracle_path(fixture_id)).unwrap(),
    )
    .unwrap();
    oracle["compile"]["workshop"].as_str().unwrap().to_string()
}

fn catalog() -> Catalog {
    Catalog::builtin().unwrap()
}

fn en() -> Locale {
    Locale::new("en-US")
}

/// Remove span suffixes so dumps can be compared modulo source locations.
fn without_spans(dump: &str) -> String {
    let re = regex::Regex::new(r" @\d+:\d+:\d+-\d+:\d+").unwrap();
    re.replace_all(dump, "").into_owned()
}

#[test]
fn emission_is_byte_stable_and_a_fixed_point() {
    let program = parser::parse(&corpus_text("synthetic/control-flow"), &catalog(), &en()).unwrap();
    let first = emitter::emit(&program, &catalog(), &en()).expect("emits");
    let second = emitter::emit(&program, &catalog(), &en()).expect("emits");
    assert_eq!(first, second, "emission must be byte-stable");

    // The emitted text is a fixed point: re-emitting the reparsed program
    // produces identical text.
    let reparsed = parser::parse(&first, &catalog(), &en()).expect("emitted text reparses");
    let reemitted = emitter::emit(&reparsed, &catalog(), &en()).expect("re-emits");
    assert_eq!(first, reemitted, "emission must be a fixed point");
}

#[test]
fn every_corpus_program_round_trips_to_equivalent_wir() {
    for fixture_id in [
        "synthetic/basic-rule",
        "synthetic/control-flow",
        "synthetic/declarations-rules",
        "synthetic/expressions-values",
        "synthetic/preprocessing",
        "real-world/overpy-cake",
    ] {
        let program = parser::parse(&corpus_text(fixture_id), &catalog(), &en())
            .unwrap_or_else(|error| panic!("{fixture_id} must parse: {error}"));
        let emitted = emitter::emit(&program, &catalog(), &en())
            .unwrap_or_else(|error| panic!("{fixture_id} must emit: {error}"));
        let reparsed = parser::parse(&emitted, &catalog(), &en()).unwrap_or_else(|error| {
            panic!("{fixture_id} emitted text must reparse:\n{error}\n{emitted}")
        });
        let original = without_spans(&program.dump());
        let round_tripped = without_spans(&reparsed.dump());
        assert_eq!(
            original, round_tripped,
            "{fixture_id} round trip must preserve semantics"
        );
    }
}

#[test]
fn emitted_text_is_recognizably_workshop() {
    let program = parser::parse(&corpus_text("synthetic/basic-rule"), &catalog(), &en()).unwrap();
    let emitted = emitter::emit(&program, &catalog(), &en()).unwrap();
    assert!(emitted.contains("rule (\"setup\") {"));
    assert!(emitted.contains("event {"));
    assert!(emitted.contains("Ongoing - Global;"));
    assert!(emitted.contains("actions {"));
    assert!(emitted.contains("Disable Inspector Recording;"));
}

#[test]
fn emitted_condition_matches_reference_infix_form() {
    let program = parser::parse(
        &corpus_text("synthetic/declarations-rules"),
        &catalog(),
        &en(),
    )
    .unwrap();
    let emitted = emitter::emit(&program, &catalog(), &en()).unwrap();
    assert!(
        emitted.contains("Has Spawned(Event Player) == True"),
        "non-comparison conditions emit in reference infix form:\n{emitted}"
    );
}

#[test]
fn debug_actions_emit_hud_text() {
    // Since M8, Debug/Print emit a semantically equivalent Create HUD Text
    // effect (documented intentional difference from the reference's
    // type-aware formatting).
    let mut program = wir::Program::default();
    let file = program
        .files
        .push(wright_ir::source::SourceFile::new("workshop.txt"));
    let value = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::Number(1.0),
        None,
    ));
    let debug = program
        .actions
        .push(wir::Action::Debug { value, span: None });
    program.rules.push(wir::Rule {
        name: "x".into(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: vec![],
        actions: vec![debug],
    });
    let _ = file;
    let emitted = emitter::emit(&program, &catalog(), &en()).expect("Debug emits");
    assert!(
        emitted.contains("Create HUD Text(All Players(All Teams), Null, Custom String(\"{0}\", 1)"),
        "debug emits the value as HUD text:\n{emitted}"
    );
    // The emitted text reparses to a createHudText action call.
    let reparsed = wright_workshop::parser::parse(&emitted, &catalog(), &en()).unwrap();
    assert_eq!(reparsed.rules.len(), 1);
}

#[test]
fn unknown_value_id_fails_explicitly() {
    let mut program = wir::Program::default();
    program
        .files
        .push(wright_ir::source::SourceFile::new("workshop.txt"));
    let value = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::Call {
            name: "notACatalogId".into(),
            args: vec![],
        },
        None,
    ));
    let call = program.actions.push(wir::Action::Call {
        name: "wait".into(),
        args: vec![value],
        span: None,
    });
    program.rules.push(wir::Rule {
        name: "x".into(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: vec![],
        actions: vec![call],
    });
    let error = emitter::emit(&program, &catalog(), &en()).expect_err("unknown id must fail");
    assert!(error.to_string().contains("notACatalogId"), "{error}");
}

use wright_ir::settings::{Settings, SettingsListElement, SettingsNode};

fn settings(children: Vec<SettingsNode>) -> Settings {
    Settings {
        span: None,
        children,
    }
}

fn group(name: &str, children: Vec<SettingsNode>) -> SettingsNode {
    SettingsNode::Group {
        name: name.to_string(),
        children,
        span: None,
    }
}

fn string(name: &str, value: &str) -> SettingsNode {
    SettingsNode::String {
        name: name.to_string(),
        value: value.to_string(),
        span: None,
    }
}

fn number(name: &str, value: f64) -> SettingsNode {
    SettingsNode::Number {
        name: name.to_string(),
        value,
        span: None,
    }
}

fn boolean(name: &str, value: bool) -> SettingsNode {
    SettingsNode::Bool {
        name: name.to_string(),
        value,
        span: None,
    }
}

fn list(name: &str, elements: &[&str]) -> SettingsNode {
    SettingsNode::List {
        name: name.to_string(),
        elements: elements
            .iter()
            .map(|value| SettingsListElement {
                value: value.to_string(),
                span: None,
            })
            .collect(),
        span: None,
    }
}

/// The pixelart settings tree (source order: assault, control, escort,
/// hybrid, skirmish).
fn pixelart_settings() -> Settings {
    let mode = |name: &str| {
        group(
            name,
            vec![
                list("enabledMaps", &[]),
                string("roleLimit", "2OfEachRolePerTeam"),
            ],
        )
    };
    settings(vec![group(
        "gamemodes",
        vec![
            mode("assault"),
            mode("control"),
            mode("escort"),
            mode("hybrid"),
            group("skirmish", vec![list("enabledMaps", &["workshopIsland"])]),
        ],
    )])
}

/// The santa settings tree (source order).
fn santa_settings() -> Settings {
    settings(vec![
        group("lobby", vec![number("ffaSlots", 6.0)]),
        group(
            "gamemodes",
            vec![
                group("ffa", vec![list("enabledMaps", &["kingsRowWinter"])]),
                group(
                    "general",
                    vec![
                        boolean("enableHeroSwitching", false),
                        string("heroLimit", "off"),
                        boolean("enableRandomHeroes", true),
                        number("respawnTime%", 30.0),
                    ],
                ),
            ],
        ),
        group(
            "heroes",
            vec![group(
                "allTeams",
                vec![
                    group(
                        "mei",
                        vec![
                            boolean("enablePrimaryFire", false),
                            boolean("enableSecondaryFire", false),
                            boolean("enableAbility1", false),
                            number("health%", 266.4),
                            boolean("enableAbility2", false),
                            number("passiveUltGen%", 0.0),
                            number("combatUltGen%", 0.0),
                        ],
                    ),
                    list("enabledHeroes", &["mei"]),
                ],
            )],
        ),
    ])
}

fn program_with_settings(settings: Settings) -> wir::Program {
    wir::Program {
        settings: Some(settings),
        ..wir::Program::default()
    }
}

/// The `settings` section of a Workshop text (the text starts with it).
fn settings_section(text: &str) -> String {
    let start = text.find("settings").expect("text has a settings section");
    let mut depth = 0usize;
    let mut end = start;
    for (index, ch) in text[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = start + index + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    text[start..end].to_string()
}

/// Collapse all whitespace for structural equality (string contents are
/// compared collapsed too; both sides render the same values).
fn collapse(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

#[test]
fn settings_emission_matches_oracle_for_pixelart() {
    let program = program_with_settings(pixelart_settings());
    let emitted = emitter::emit(&program, &catalog(), &en()).expect("emits");
    let oracle = corpus_text("real-world/overpy-pixelart");
    assert_eq!(
        collapse(&settings_section(&emitted)),
        collapse(&settings_section(&oracle)),
        "emitted settings section must match the oracle region (whitespace-collapsed)"
    );
}

#[test]
fn settings_emission_matches_oracle_for_santa() {
    let program = program_with_settings(santa_settings());
    let emitted = emitter::emit(&program, &catalog(), &en()).expect("emits");
    let oracle = corpus_text("real-world/overpy-santa");
    assert_eq!(
        collapse(&settings_section(&emitted)),
        collapse(&settings_section(&oracle)),
        "emitted settings section must match the oracle region (whitespace-collapsed)"
    );
}

#[test]
fn settings_emission_is_deterministic() {
    let program = program_with_settings(santa_settings());
    let first = emitter::emit(&program, &catalog(), &en()).expect("emits");
    let second = emitter::emit(&program, &catalog(), &en()).expect("emits");
    assert_eq!(first, second);
}

#[test]
fn settings_free_program_emits_no_settings_section() {
    let mut program = wir::Program::default();
    let rule = program.rules.push(wir::Rule {
        name: "x".into(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: vec![],
        actions: vec![],
    });
    let _ = rule;
    let emitted = emitter::emit(&program, &catalog(), &en()).expect("emits");
    assert!(
        !emitted.contains("settings"),
        "settings-free programs emit no settings section:\n{emitted}"
    );
}

#[test]
fn settings_emission_is_rejected_by_the_workshop_parser() {
    // Roundtrip boundary (#86): the ws parser never learns the settings
    // section, so a settings-bearing emission cannot reparse.
    let program = program_with_settings(pixelart_settings());
    let emitted = emitter::emit(&program, &catalog(), &en()).expect("emits");
    assert!(emitted.starts_with("settings {"));
    assert!(
        parser::parse(&emitted, &catalog(), &en()).is_err(),
        "settings-bearing emission must be rejected by the ws parser"
    );
}

#[test]
fn enabled_false_prefixes_the_mode_header() {
    let program = program_with_settings(settings(vec![group(
        "gamemodes",
        vec![group(
            "assault",
            vec![
                boolean("enabled", false),
                boolean("enableCompetitiveRules", true),
            ],
        )],
    )]));
    let emitted = emitter::emit(&program, &catalog(), &en()).expect("emits");
    let section = settings_section(&emitted);
    assert!(collapse(&section).contains("disabledAssault{CompetitiveRules:On}"));
}

#[test]
fn empty_list_emits_empty_braces_block() {
    let program = program_with_settings(settings(vec![group(
        "gamemodes",
        vec![group("skirmish", vec![list("enabledMaps", &[])])],
    )]));
    let emitted = emitter::emit(&program, &catalog(), &en()).expect("emits");
    let section = settings_section(&emitted);
    assert!(
        collapse(&section).contains("Skirmish{enabledmaps{}}"),
        "empty lists emit an empty braces block:\n{section}"
    );
}

#[test]
fn settings_section_precedes_variables() {
    let mut program = program_with_settings(santa_settings());
    program.global_variables.push(wir::WorkshopVariable {
        name: "x".into(),
        index: 0,
        span: None,
        name_span: None,
        initializer: None,
    });
    let emitted = emitter::emit(&program, &catalog(), &en()).expect("emits");
    let settings_pos = emitted.find("settings").unwrap();
    let variables_pos = emitted.find("variables {").unwrap();
    assert!(settings_pos < variables_pos, "settings precedes variables");
}

#[test]
fn percent_keys_append_the_suffix() {
    let program = program_with_settings(settings(vec![group(
        "gamemodes",
        vec![group("general", vec![number("respawnTime%", 30.0)])],
    )]));
    let emitted = emitter::emit(&program, &catalog(), &en()).expect("emits");
    assert!(collapse(&settings_section(&emitted)).contains("RespawnTimeScalar:30%"));
}

#[test]
fn string_values_are_escaped() {
    let program = program_with_settings(settings(vec![group(
        "main",
        vec![string("description", "a \"quoted\" line")],
    )]));
    let emitted = emitter::emit(&program, &catalog(), &en()).expect("emits");
    assert!(
        collapse(&settings_section(&emitted)).contains("Description:\"a\\\"quoted\\\"line\""),
        "strings are escaped: {emitted}"
    );
}

#[test]
fn settings_strings_re_escape_decoded_escapes() {
    // A decoded `\n` (and other JSONC escapes) must round-trip to the
    // oracle's literal two-character spelling, not a raw byte (evidence: the
    // inputhud description, #86).
    let program = program_with_settings(settings(vec![group(
        "main",
        vec![string("description", "line one\nline two\t\"quoted\"\\end")],
    )]));
    let emitted = emitter::emit(&program, &catalog(), &en()).expect("emits");
    let section = settings_section(&emitted);
    assert!(
        section.contains("Description: \"line one\\nline two\\t\\\"quoted\\\"\\\\end\""),
        "decoded escapes re-escape to the oracle spelling: {section}"
    );
    assert!(
        !section.contains('\u{000A}') || !section.contains("Description: \"line one\n"),
        "no raw newline byte inside the settings string: {section}"
    );
}
