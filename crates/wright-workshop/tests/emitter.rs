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
    // Corpus text parses against the catalog context (expected enum domains
    // come from the canonical catalog signatures). `real-world/overpy-cake`
    // is the documented exception: its bare `Up` (OverPy folds the vector-up
    // constant inside `Add(...)`) is genuinely ambiguous between the `Vector`
    // and `Rounding` enum domains and no enclosing signature pins it, so the
    // parser rejects it deterministically (#111).
    let documented_ambiguities = [("real-world/overpy-cake", "ambiguous enum member 'Up'")];
    for fixture_id in [
        "synthetic/basic-rule",
        "synthetic/control-flow",
        "synthetic/declarations-rules",
        "synthetic/expressions-values",
        "synthetic/preprocessing",
        "synthetic/receiver-calls",
        "real-world/overpy-cake",
    ] {
        let catalog = catalog();
        let program =
            match parser::parse_with_context(&corpus_text(fixture_id), &catalog, &en(), &catalog) {
                Ok(program) => program,
                Err(error) => {
                    let Some((_, message)) = documented_ambiguities
                        .iter()
                        .find(|(id, _)| **id == *fixture_id)
                    else {
                        panic!("{fixture_id} must parse: {error}");
                    };
                    assert!(
                        error.to_string().contains(message),
                        "{fixture_id} fails only with the documented ambiguity, got: {error}"
                    );
                    continue;
                }
            };
        let emitted = emitter::emit(&program, &catalog, &en())
            .unwrap_or_else(|error| panic!("{fixture_id} must emit: {error}"));
        let reparsed = parser::parse_with_context(&emitted, &catalog, &en(), &catalog)
            .unwrap_or_else(|error| {
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
    let catalog = catalog();
    let program = parser::parse_with_context(
        &corpus_text("synthetic/declarations-rules"),
        &catalog,
        &en(),
        &catalog,
    )
    .unwrap();
    let emitted = emitter::emit(&program, &catalog, &en()).unwrap();
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
        wright_ir::wir::Value::Number {
            value: 1.0,
            text: "1".to_string(),
        },
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
    let catalog = catalog();
    let reparsed =
        wright_workshop::parser::parse_with_context(&emitted, &catalog, &en(), &catalog).unwrap();
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

#[test]
fn rule_final_if_omits_the_trailing_end_and_round_trips() {
    // Amended AC-12: a rule-final if/if-else closes with the `}` (no
    // trailing `End;`), byte-equal to the oracle's spelling; the ws parser
    // accepts it and re-emission is a byte-identical fixed point. The
    // middle-of-rule if keeps `End;`.
    let oracle_spelling = r#"variables {
    global:
        0: x
}

rule ("r") {
    event {
        Ongoing - Global;
    }
    actions {
        If(Compare(Global.x, ==, 1));
            Disable Inspector Recording;
        Else;
            Disable Inspector Recording;
    }
}

"#;
    let program = parser::parse(oracle_spelling, &catalog(), &en())
        .expect("the oracle's rule-final if spelling must parse");
    let emitted = emitter::emit(&program, &catalog(), &en()).expect("emits");
    assert_eq!(
        emitted, oracle_spelling,
        "the rule-final if-else re-emits byte-identically"
    );
    assert!(
        !emitted.contains("End;"),
        "no trailing End; in the rule-final if-else"
    );
    // Middle-of-rule if keeps End;.
    let middle = parser::parse(
        r#"variables {
    global:
        0: x
}

rule ("r") {
    event {
        Ongoing - Global;
    }
    actions {
        If(Compare(Global.x, ==, 1));
            Disable Inspector Recording;
        End;
        Set Global Variable(x, 2);
    }
}

"#,
        &catalog(),
        &en(),
    )
    .expect("middle-of-rule if parses");
    let emitted = emitter::emit(&middle, &catalog(), &en()).expect("emits");
    assert!(emitted.contains("End;"), "middle-of-rule if keeps End;");
}

#[test]
fn constant_format_calls_fold_to_the_substituted_text() {
    // Amended AC-13: all-constant `.format()` calls fold into the
    // substituted Custom String text (oracle spelling), including the
    // two-decimal float rendering; variable arguments stay as format nodes.
    let mut program = wir::Program::default();
    let file = program
        .files
        .push(wright_ir::source::SourceFile::new("workshop.txt"));
    let text = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::String("value: {0}".to_string()),
        None,
    ));
    let three = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::Number {
            value: 3.0,
            text: "3".to_string(),
        },
        None,
    ));
    let folded = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::Call {
            name: "format".to_string(),
            args: vec![text, three],
        },
        None,
    ));
    let action = program.actions.push(wir::Action::SetGlobalVariable {
        variable: wright_ir::ids::Id::from_index(0),
        value: folded,
        span: None,
        target_span: None,
    });
    program.global_variables.push(wir::WorkshopVariable {
        name: "y".into(),
        index: 0,
        span: None,
        name_span: None,
    });
    program.rules.push(wir::Rule {
        name: "r".into(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: vec![],
        actions: vec![action],
    });
    let _ = file;
    let emitted = emitter::emit(&program, &catalog(), &en()).expect("emits");
    assert!(
        emitted.contains("Set Global Variable(y, Custom String(\"value: 3\"));"),
        "constant format folds to the substituted text: {emitted}"
    );
    let reparsed = parser::parse(&emitted, &catalog(), &en()).expect("folded output reparses");
    let reemitted = emitter::emit(&reparsed, &catalog(), &en()).expect("re-emits");
    assert_eq!(
        emitted, reemitted,
        "folded format must be a byte-identical fixed point"
    );
}

#[test]
fn constant_float_format_arguments_use_two_decimals() {
    // The oracle folds 0.5 to `0.50` and 0.125 to `0.13` (JS toFixed(2)).
    let mut program = wir::Program::default();
    let file = program
        .files
        .push(wright_ir::source::SourceFile::new("workshop.txt"));
    let text = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::String("v: {0}".to_string()),
        None,
    ));
    let half = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::Number {
            value: 0.5,
            text: "0.5".to_string(),
        },
        None,
    ));
    let folded = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::Call {
            name: "format".to_string(),
            args: vec![text, half],
        },
        None,
    ));
    let action = program.actions.push(wir::Action::SetGlobalVariable {
        variable: wright_ir::ids::Id::from_index(0),
        value: folded,
        span: None,
        target_span: None,
    });
    program.global_variables.push(wir::WorkshopVariable {
        name: "y".into(),
        index: 0,
        span: None,
        name_span: None,
    });
    program.rules.push(wir::Rule {
        name: "r".into(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: vec![],
        actions: vec![action],
    });
    let _ = file;
    let emitted = emitter::emit(&program, &catalog(), &en()).expect("emits");
    assert!(
        emitted.contains("Set Global Variable(y, Custom String(\"v: 0.50\"));"),
        "0.5 folds to 0.50 (toFixed(2)): {emitted}"
    );
}

#[test]
fn split_and_reescaped_value_strings_round_trip_byte_identically() {
    // Amended AC-6: a long string (split continuation chain) and an escaped
    // string parse and re-emit byte-identically through the ws parser.
    let mut program = wir::Program::default();
    let file = program
        .files
        .push(wright_ir::source::SourceFile::new("workshop.txt"));
    let long = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::String("B".repeat(300)),
        None,
    ));
    let escaped = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::String("a\nb\"c\\d\te".to_string()),
        None,
    ));
    let first = program.actions.push(wir::Action::SetGlobalVariable {
        variable: wright_ir::ids::Id::from_index(0),
        value: long,
        span: None,
        target_span: None,
    });
    let second = program.actions.push(wir::Action::SetGlobalVariable {
        variable: wright_ir::ids::Id::from_index(1),
        value: escaped,
        span: None,
        target_span: None,
    });
    program.global_variables.push(wir::WorkshopVariable {
        name: "x".into(),
        index: 0,
        span: None,
        name_span: None,
    });
    program.global_variables.push(wir::WorkshopVariable {
        name: "y".into(),
        index: 1,
        span: None,
        name_span: None,
    });
    program.rules.push(wir::Rule {
        name: "r".into(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: vec![],
        actions: vec![first, second],
    });
    let _ = file;
    let emitted = emitter::emit(&program, &catalog(), &en()).expect("emits");
    assert!(
        emitted.contains("Custom String(\"B",),
        "the long string splits into a continuation chain: {emitted}"
    );
    assert!(
        emitted.contains("Custom String(\"a\\nb\\\"c\\\\d\te\")"),
        "escapes re-emit in the oracle spelling: {emitted}"
    );
    let reparsed = parser::parse(&emitted, &catalog(), &en())
        .expect("the split chain and escaped string must reparse");
    let reemitted = emitter::emit(&reparsed, &catalog(), &en()).expect("re-emits");
    assert_eq!(
        emitted, reemitted,
        "split and re-escaped spellings must be a byte-identical fixed point"
    );
}

#[test]
fn implicit_format_placeholders_renumber_to_the_oracle_form() {
    // Amended AC-15: implicit `{}` placeholders renumber positionally to the
    // oracle's explicit form (`"v: {}".format(x)` -> `Custom String("v:
    // {0}", Global.x)`), and constant arguments fold into the text with the
    // remaining placeholders renumbered (`"{} {}".format(3, x)` ->
    // `Custom String("3 {0}", Global.x)`). Both byte-quoted oracle pins.
    let mut program = wir::Program::default();
    let file = program
        .files
        .push(wright_ir::source::SourceFile::new("workshop.txt"));
    let text = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::String("v: {}".to_string()),
        None,
    ));
    let variable = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::GlobalVariable(wright_ir::ids::Id::from_index(0)),
        None,
    ));
    let call = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::Call {
            name: "format".to_string(),
            args: vec![text, variable],
        },
        None,
    ));
    let action = program.actions.push(wir::Action::SetGlobalVariable {
        variable: wright_ir::ids::Id::from_index(1),
        value: call,
        span: None,
        target_span: None,
    });
    program.global_variables.push(wir::WorkshopVariable {
        name: "x".into(),
        index: 0,
        span: None,
        name_span: None,
    });
    program.global_variables.push(wir::WorkshopVariable {
        name: "z".into(),
        index: 1,
        span: None,
        name_span: None,
    });
    program.rules.push(wir::Rule {
        name: "r".into(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: vec![],
        actions: vec![action],
    });
    let _ = file;
    let emitted = emitter::emit(&program, &catalog(), &en()).expect("emits");
    assert!(
        emitted.contains("Set Global Variable(z, Custom String(\"v: {0}\", Global.x));"),
        "implicit placeholders renumber to the oracle form: {emitted}"
    );
    let reparsed = parser::parse(&emitted, &catalog(), &en()).expect("reparses");
    let reemitted = emitter::emit(&reparsed, &catalog(), &en()).expect("re-emits");
    assert_eq!(
        emitted, reemitted,
        "renumbered format must be a byte-identical fixed point"
    );
}

#[test]
fn partial_constant_format_folds_and_renumbers() {
    // The oracle folds the constant into the text and renumbers the
    // remaining placeholder: `"{} {}".format(3, x)` ->
    // `Custom String("3 {0}", Global.x)` (byte-quoted pin).
    let mut program = wir::Program::default();
    let file = program
        .files
        .push(wright_ir::source::SourceFile::new("workshop.txt"));
    let text = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::String("{} {}".to_string()),
        None,
    ));
    let three = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::Number {
            value: 3.0,
            text: "3".to_string(),
        },
        None,
    ));
    let variable = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::GlobalVariable(wright_ir::ids::Id::from_index(0)),
        None,
    ));
    let call = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::Call {
            name: "format".to_string(),
            args: vec![text, three, variable],
        },
        None,
    ));
    let action = program.actions.push(wir::Action::SetGlobalVariable {
        variable: wright_ir::ids::Id::from_index(1),
        value: call,
        span: None,
        target_span: None,
    });
    program.global_variables.push(wir::WorkshopVariable {
        name: "x".into(),
        index: 0,
        span: None,
        name_span: None,
    });
    program.global_variables.push(wir::WorkshopVariable {
        name: "z".into(),
        index: 1,
        span: None,
        name_span: None,
    });
    program.rules.push(wir::Rule {
        name: "r".into(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: vec![],
        actions: vec![action],
    });
    let _ = file;
    let emitted = emitter::emit(&program, &catalog(), &en()).expect("emits");
    assert!(
        emitted.contains("Set Global Variable(z, Custom String(\"3 {0}\", Global.x));"),
        "the constant folds and the variable placeholder renumbers: {emitted}"
    );
}

#[test]
fn playervar_reads_parenthesize_the_receiver() {
    // Amended AC-16: `g = eventPlayer.p` emits `Set Global Variable(g,
    // (Event Player).p)` (byte-quoted oracle pin) and the spelling
    // round-trips through the ws parser.
    let mut program = wir::Program::default();
    let file = program
        .files
        .push(wright_ir::source::SourceFile::new("workshop.txt"));
    let player = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::EventPlayer,
        None,
    ));
    let read = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::PlayerVariable {
            player,
            variable: wright_ir::ids::Id::from_index(0),
        },
        None,
    ));
    let action = program.actions.push(wir::Action::SetGlobalVariable {
        variable: wright_ir::ids::Id::from_index(0),
        value: read,
        span: None,
        target_span: None,
    });
    program.global_variables.push(wir::WorkshopVariable {
        name: "g".into(),
        index: 0,
        span: None,
        name_span: None,
    });
    program.player_variables.push(wir::WorkshopVariable {
        name: "p".into(),
        index: 0,
        span: None,
        name_span: None,
    });
    program.rules.push(wir::Rule {
        name: "r".into(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::Global,
        conditions: vec![],
        actions: vec![action],
    });
    let _ = file;
    let emitted = emitter::emit(&program, &catalog(), &en()).expect("emits");
    assert!(
        emitted.contains("Set Global Variable(g, (Event Player).p);"),
        "playervar reads parenthesize the receiver: {emitted}"
    );
    let reparsed =
        parser::parse(&emitted, &catalog(), &en()).expect("the oracle spelling reparses");
    let reemitted = emitter::emit(&reparsed, &catalog(), &en()).expect("re-emits");
    assert_eq!(
        emitted, reemitted,
        "playervar reads must be a byte-identical fixed point"
    );
}

#[test]
fn receiver_call_actions_and_values_emit_catalog_spellings() {
    // Issue #104: `.opy` receiver calls lower to `Action::Call`/`Value::Call`
    // whose `name` is the receiver method; emission resolves those names
    // through the catalog (general path, no per-name special cases).
    let mut program = wir::Program::default();
    program
        .files
        .push(wright_ir::source::SourceFile::new("workshop.txt"));
    let player = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::EventPlayer,
        None,
    ));
    let percent = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::Number {
            value: 100.0,
            text: "100".to_string(),
        },
        None,
    ));
    let alive = program.values.push(wright_ir::wir::ValueNode::new(
        wright_ir::wir::Value::Call {
            name: "isAlive".to_string(),
            args: vec![player],
        },
        None,
    ));
    let move_speed = program.actions.push(wir::Action::Call {
        name: "setMoveSpeed".to_string(),
        args: vec![player, percent],
        span: None,
    });
    program.rules.push(wir::Rule {
        name: "r".into(),
        span: None,
        name_span: None,
        disabled: false,
        event: wir::Event::EachPlayer,
        conditions: vec![alive],
        actions: vec![move_speed],
    });

    let emitted = emitter::emit(&program, &catalog(), &en()).expect("emits");
    assert!(
        emitted.contains("Is Alive(Event Player) == True;"),
        "receiver value calls resolve through the catalog: {emitted}"
    );
    assert!(
        emitted.contains("Set Move Speed(Event Player, 100);"),
        "receiver action calls resolve through the catalog: {emitted}"
    );
}
