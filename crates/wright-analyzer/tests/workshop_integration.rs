use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use workshop_rs::catalog::Locale;
use workshop_rs::parser;
use workshop_rs::{
    Action, Condition, Event, EventTarget, EventTeam, PlayerEventKind, Program, Rule,
    Value as WorkshopValue, Variable,
};
use wright_analyzer::analysis::Severity;
use wright_analyzer::canonical::analyze;
use wright_analyzer::registry::{LintConfig, LintRegistry};
use wright_analyzer::service::{Origin, SemanticService};

fn workshop_path(fixture_id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/workshop")
        .join(fixture_id)
        .with_extension("ws")
}

fn fixture_text(fixture_id: &str) -> String {
    std::fs::read_to_string(workshop_path(fixture_id)).unwrap()
}

fn workshop_service(fixture_id: &str) -> SemanticService<'static> {
    let text = fixture_text(fixture_id);
    workshop_service_from_text(&text)
}

fn workshop_origin() -> Origin {
    Origin {
        kind: "workshop".to_string(),
        locale: Some(Locale::new("en-US").to_string()),
    }
}

fn service_for(program: Program) -> SemanticService<'static> {
    SemanticService::with_shared_program(
        Arc::new(program),
        Origin {
            kind: "unknown".to_string(),
            locale: None,
        },
        LintConfig::default(),
        Arc::new(LintRegistry::default()),
    )
}

fn workshop_service_from_text(text: &str) -> SemanticService<'static> {
    let catalog = workshop_rs::catalog::Catalog::builtin_ref().unwrap();
    let program = parser::parse_with_context(text, catalog, &Locale::new("en-US"), catalog)
        .unwrap_or_else(|error| panic!("Workshop source must parse: {error}"));
    SemanticService::with_shared_program(
        Arc::new(program),
        workshop_origin(),
        LintConfig::default(),
        Arc::new(LintRegistry::default()),
    )
}

fn query(service: &SemanticService<'_>, request: Value) -> Value {
    let response: Value =
        serde_json::from_str(&service.handle_json(&serde_json::to_string(&request).unwrap()))
            .unwrap();
    assert!(
        response.get("error").is_none(),
        "query must succeed: {response}"
    );
    response["result"].clone()
}

fn id_for(service: &SemanticService<'_>, request: Value, name: &str) -> u32 {
    query(service, request)
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == name)
        .and_then(|item| item["id"].as_u64())
        .unwrap_or_else(|| panic!("query result has no item named {name}")) as u32
}

fn span_text(source: &str, span: &Value) -> String {
    let (start, end) = (&span["start"], &span["end"]);
    assert_eq!(
        start["line"], end["line"],
        "test spans stay on one line: {span}"
    );
    let line = source
        .lines()
        .nth(start["line"].as_u64().unwrap() as usize - 1)
        .unwrap();
    let (start_col, end_col) = (
        start["col"].as_u64().unwrap() as usize,
        end["col"].as_u64().unwrap() as usize,
    );
    line.chars()
        .skip(start_col - 1)
        .take(end_col - start_col)
        .collect()
}

#[test]
fn workshop_input_runs_all_semantic_queries() {
    let service = workshop_service("synthetic/control-flow");
    let rule = id_for(
        &service,
        serde_json::json!({"op": "listRules"}),
        "bounded while",
    );
    let symbol = id_for(&service, serde_json::json!({"op": "listSymbols"}), "index");
    let requests = [
        serde_json::json!({"op": "program"}),
        serde_json::json!({"op": "listRules"}),
        serde_json::json!({"op": "getRule", "rule": rule}),
        serde_json::json!({"op": "findReferences", "symbol": symbol}),
        serde_json::json!({"op": "getUsage", "symbol": symbol}),
        serde_json::json!({"op": "getCfg", "rule": rule}),
        serde_json::json!({"op": "getFindings"}),
    ];
    for request in requests {
        query(&service, request);
    }

    let program_response: Value =
        serde_json::from_str(&service.handle_json(r#"{"op":"program"}"#)).unwrap();
    assert_eq!(program_response["result"]["origin"]["kind"], "workshop");
    assert_eq!(program_response["result"]["origin"]["locale"], "en-us");
}

#[test]
fn player_event_queries_keep_catalog_event_ids() {
    let mut program = Program::new();
    program.rule(Rule::new(
        "dealt damage",
        Event::Player {
            kind: PlayerEventKind::DealtDamage,
            team: EventTeam::All,
            target: EventTarget::All,
        },
    ));
    let service = service_for(program);
    let rule = query(&service, serde_json::json!({"op": "getRule", "rule": 0}));
    assert_eq!(rule["event"], "playerDealtDamage");
}

#[test]
fn workshop_input_analysis_findings_are_available() {
    let service = workshop_service("synthetic/control-flow");
    let response: Value =
        serde_json::from_str(&service.handle_json(r#"{"op":"getFindings"}"#)).unwrap();
    let findings = response["result"].as_array().unwrap();
    assert!(
        findings
            .iter()
            .any(|finding| finding["code"] == "min-wait-loop"),
        "the Workshop-origin bounded-while is a hot loop"
    );
    for finding in findings {
        assert!(
            finding["span"].is_object(),
            "findings must carry Workshop source spans: {finding}"
        );
    }
}

#[test]
fn workshop_analysis_preserves_lint_semantics() {
    let source = r#"
variables {
    global:
        0: index
        1: other
    player:
        0: playerIndex
}
rule ("analysis parity") {
    event {
        Ongoing - Global;
    }
    conditions {
        Compare(Global.index, ==, 0);
        Compare(Distance Between(Event Player, Event Player), <, 5);
    }
    actions {
        If(Compare(Global.index, ==, 0));
        Else If(Compare(Global.index, ==, 0));
        End;
        While(Compare(Global.index, ==, 0));
            Wait(0.016, Ignore Condition);
        End;
        While(True);
            Modify Global Variable(other, Add, 1);
        End;
        While(Compare(Global.index, <, 3));
            Modify Global Variable(other, Add, 1);
        End;
        While(Compare(Global.index, <, 3));
            Modify Global Variable(index, Add, 1);
        End;
        While(Compare(Global.index, <, 3));
            Modify Global Variable(index, Subtract, 1);
        End;
        While(Compare(Global.index, <, 3));
            Set Global Variable(other, Distance Between(Position Of(Event Player), Vector(0, 0, 0)));
            Set Global Variable(other, Distance Between(Position Of(Event Player), Vector(0, 0, 0)));
        End;
        For Player Variable(Event Player, playerIndex, 0, 1, 1);
            Wait(0.016, Ignore Condition);
            Set Global Variable(other, Distance Between(Position Of(Event Player), Vector(0, 0, 0)));
        End;
    }
}
"#;
    let service = workshop_service_from_text(source);
    let findings = query(&service, serde_json::json!({"op": "getFindings"}));
    let findings = findings.as_array().unwrap();

    assert!(findings.iter().any(|finding| {
        finding["code"] == "ongoing-condition-hot-path"
            && finding["message"]
                .as_str()
                .unwrap()
                .contains("condition 2 of 2")
    }));
    let duplicates: Vec<_> = findings
        .iter()
        .filter(|finding| finding["code"] == "duplicate-condition")
        .collect();
    assert_eq!(
        duplicates.len(),
        1,
        "only the in-chain Else If repeats an earlier branch condition"
    );
    assert_eq!(duplicates[0]["action"], 1);
    let minimum_waits: Vec<_> = findings
        .iter()
        .filter(|finding| finding["code"] == "min-wait-loop")
        .collect();
    assert_eq!(minimum_waits.len(), 2);
    assert!(minimum_waits.iter().all(
        |finding| finding["severity"] == "warning" && finding["evidence"] == "static-indicator"
    ));

    let expensive: Vec<_> = findings
        .iter()
        .filter(|finding| finding["code"] == "expensive-loop-check")
        .collect();
    assert_eq!(expensive.len(), 3);
    assert!(
        expensive
            .iter()
            .all(|finding| { finding["severity"] == "info" && finding["evidence"] == "heuristic" })
    );

    let repeated = findings
        .iter()
        .find(|finding| finding["code"] == "repeated-value")
        .unwrap();
    let repeated_message = repeated["message"].as_str().unwrap();
    assert!(repeated_message.contains("evaluated 2 times"));
    assert!(repeated_message.contains("`distance(getPosition(eventPlayer), vector(0, 0, 0))`"));
    assert_eq!(repeated["severity"], "warning");
    assert_eq!(repeated["evidence"], "exact");

    let ongoing = findings
        .iter()
        .find(|finding| finding["code"] == "ongoing-condition-hot-path")
        .unwrap();
    assert_eq!(ongoing["severity"], "info");
    assert_eq!(ongoing["evidence"], "heuristic");

    let no_yield: Vec<_> = findings
        .iter()
        .filter(|finding| finding["code"] == "while-without-wait")
        .collect();
    let boundedness: Vec<_> = no_yield
        .iter()
        .map(|finding| finding["boundedness"].as_str().unwrap())
        .collect();
    assert_eq!(
        boundedness,
        [
            "obviously-unbounded",
            "unknown",
            "statically-bounded",
            "unknown",
            "unknown"
        ]
    );
    let severities: Vec<_> = no_yield
        .iter()
        .map(|finding| finding["severity"].as_str().unwrap())
        .collect();
    assert_eq!(
        severities,
        ["warning", "warning", "info", "warning", "warning"]
    );
    assert!(
        no_yield[0]["message"]
            .as_str()
            .unwrap()
            .contains("statically true")
    );
    assert!(
        no_yield[1]["message"]
            .as_str()
            .unwrap()
            .contains("boundedness is unknown")
    );
    assert!(
        no_yield[2]["message"]
            .as_str()
            .unwrap()
            .contains("finite number")
    );

    let descriptors = query(&service, serde_json::json!({"op": "lintRules"}));
    let descriptors = descriptors["rules"].as_array().unwrap();
    for (id, evidence) in [
        ("ongoing-condition-hot-path", "heuristic"),
        ("repeated-value", "exact"),
        ("expensive-loop-check", "heuristic"),
    ] {
        assert!(
            descriptors
                .iter()
                .any(|descriptor| { descriptor["id"] == id && descriptor["evidence"] == evidence })
        );
    }
}

fn duplicate_condition_results(source: &str) -> Vec<Value> {
    let service = workshop_service_from_text(source);
    let findings = query(&service, serde_json::json!({"op": "getFindings"}));
    findings
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["code"] == "duplicate-condition")
        .cloned()
        .collect()
}

#[test]
fn duplicate_condition_ignores_independent_if_blocks() {
    let source = r#"
variables {
    global:
        0: index
        1: other
}
rule ("two independent ifs") {
    event {
        Ongoing - Global;
    }
    actions {
        If(Compare(Global.index, ==, 0));
            Set Global Variable(other, 1);
        End;
        If(Compare(Global.index, ==, 0));
            Set Global Variable(other, 2);
        End;
    }
}
"#;
    assert!(
        duplicate_condition_results(source).is_empty(),
        "a second If block is evaluated independently and is reachable"
    );
}

#[test]
fn duplicate_condition_ignores_while_and_if_across_blocks() {
    let source = r#"
variables {
    global:
        0: index
        1: other
}
rule ("if and while blocks") {
    event {
        Ongoing - Global;
    }
    actions {
        While(Compare(Global.index, ==, 0));
            Wait(0.016, Ignore Condition);
        End;
        If(Compare(Global.index, ==, 0));
            Set Global Variable(other, 1);
        End;
        While(Compare(Global.index, ==, 0));
            Wait(0.016, Ignore Condition);
        End;
    }
}
"#;
    assert!(
        duplicate_condition_results(source).is_empty(),
        "While conditions are re-evaluated per iteration and never compared"
    );
}

#[test]
fn duplicate_condition_flags_repeat_within_one_chain() {
    let source = r#"
variables {
    global:
        0: index
        1: other
}
rule ("repeated else if") {
    event {
        Ongoing - Global;
    }
    actions {
        If(Compare(Global.index, ==, 0));
            Set Global Variable(other, 1);
        Else If(Compare(Global.index, ==, 0));
            Set Global Variable(other, 2);
        End;
    }
}
"#;
    let findings = duplicate_condition_results(source);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["action"], 2);
    assert_eq!(findings[0]["evidence"], "exact");
}

#[test]
fn duplicate_condition_flags_repeat_against_earlier_chain_branch() {
    let source = r#"
variables {
    global:
        0: index
        1: other
}
rule ("repeat against first branch") {
    event {
        Ongoing - Global;
    }
    actions {
        If(Compare(Global.index, ==, 0));
            Set Global Variable(other, 1);
        Else If(Compare(Global.index, ==, 1));
            Set Global Variable(other, 2);
        Else If(Compare(Global.index, ==, 0));
            Set Global Variable(other, 3);
        End;
    }
}
"#;
    let findings = duplicate_condition_results(source);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["action"], 4);
}

#[test]
fn duplicate_condition_does_not_compare_across_chains() {
    let source = r#"
variables {
    global:
        0: index
        1: other
}
rule ("same else if in two chains") {
    event {
        Ongoing - Global;
    }
    actions {
        If(Compare(Global.index, ==, 0));
            Set Global Variable(other, 1);
        Else If(Compare(Global.index, ==, 1));
            Set Global Variable(other, 2);
        End;
        If(Compare(Global.index, ==, 2));
            Set Global Variable(other, 3);
        Else If(Compare(Global.index, ==, 1));
            Set Global Variable(other, 4);
        End;
    }
}
"#;
    assert!(
        duplicate_condition_results(source).is_empty(),
        "identical Else If conditions in different chains are both reachable"
    );
}

#[test]
fn duplicate_condition_scopes_nested_chains() {
    let source = r#"
variables {
    global:
        0: index
        1: other
}
rule ("nested chains") {
    event {
        Ongoing - Global;
    }
    actions {
        If(Compare(Global.index, ==, 0));
            If(Compare(Global.index, ==, 1));
                Set Global Variable(other, 1);
            Else If(Compare(Global.index, ==, 0));
                Set Global Variable(other, 2);
            End;
        Else If(Compare(Global.index, ==, 0));
            Set Global Variable(other, 3);
        End;
    }
}
"#;
    let findings = duplicate_condition_results(source);
    assert_eq!(
        findings.len(),
        1,
        "the inner Else If repeats nothing in its own chain; only the outer \
         Else If repeats the outer If"
    );
    assert_eq!(findings[0]["action"], 6);
}

#[test]
fn duplicate_condition_chain_continues_past_nested_loop() {
    let source = r#"
variables {
    global:
        0: index
        1: other
}
rule ("loop inside chain body") {
    event {
        Ongoing - Global;
    }
    actions {
        If(Compare(Global.index, ==, 0));
            While(Compare(Global.index, ==, 1));
                Wait(0.016, Ignore Condition);
            End;
        Else If(Compare(Global.index, ==, 0));
            Set Global Variable(other, 1);
        End;
    }
}
"#;
    let findings = duplicate_condition_results(source);
    assert_eq!(
        findings.len(),
        1,
        "a loop inside the chain body must not close the chain"
    );
    assert_eq!(findings[0]["action"], 4);
}

#[test]
fn duplicate_condition_tracks_disabled_blocks() {
    let source = r#"
variables {
    global:
        0: index
        1: other
}
rule ("disabled blocks") {
    event {
        Ongoing - Global;
    }
    actions {
        disabled If(Compare(Global.index, ==, 0));
            Set Global Variable(other, 1);
        End;
        If(Compare(Global.index, ==, 0));
            disabled While(Compare(Global.index, ==, 1));
                Wait(0.016, Ignore Condition);
            End;
        Else If(Compare(Global.index, ==, 0));
            Set Global Variable(other, 2);
        End;
    }
}
"#;
    let findings = duplicate_condition_results(source);
    assert_eq!(
        findings.len(),
        1,
        "disabled blocks still occupy their `End`; only the enabled chain's \
         own repeat is reported"
    );
    assert_eq!(findings[0]["action"], 7);
}

#[test]
fn disabled_block_body_stays_dead_through_its_own_end() {
    let source = r#"
rule ("outer loop") {
    event {
        Ongoing - Global;
    }
    actions {
        While(True);
            disabled If(True);
                Wait(0.016, Ignore Condition);
            End;
            Wait(1, Ignore Condition);
        End;
    }
}
"#;
    let service = workshop_service_from_text(source);
    let findings = query(&service, serde_json::json!({"op": "getFindings"}));
    let findings = findings.as_array().unwrap();
    assert!(
        !findings
            .iter()
            .any(|finding| finding["code"] == "while-without-wait"),
        "the live Wait keeps the loop out of while-without-wait: {findings:?}"
    );

    let rule = id_for(
        &service,
        serde_json::json!({"op": "listRules"}),
        "outer loop",
    );
    let cfg = query(&service, serde_json::json!({"op": "getCfg", "rule": rule}));
    let blocks = cfg["blocks"].as_array().unwrap();
    let header = blocks
        .iter()
        .find(|block| block["kind"] == "while")
        .expect("the live While produces a loop header");
    assert_eq!(header["actions"], serde_json::json!([0]));
    let body = header["successors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|edge| edge["kind"] == "true")
        .map(|edge| &blocks[edge["to"].as_u64().unwrap() as usize])
        .expect("the loop header has a body edge");
    assert_eq!(body["waits"], true, "the loop body holds the live Wait");
    assert!(
        body["actions"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!(4)),
        "the loop body contains the live Wait at index 4: {body:?}"
    );
    for dead in [1usize, 2, 3] {
        assert!(
            !blocks.iter().any(|block| block["actions"]
                .as_array()
                .is_some_and(|actions| actions.contains(&serde_json::json!(dead)))),
            "the disabled If region's action {dead} is in no CFG block"
        );
    }
}

#[test]
fn a_disabled_wait_is_not_live_for_loop_checks() {
    let source = r#"
rule ("disabled wait") {
    event {
        Ongoing - Global;
    }
    actions {
        While(True);
            disabled If(True);
                Wait(0.016, Ignore Condition);
            End;
        End;
    }
}
"#;
    let service = workshop_service_from_text(source);
    let findings = query(&service, serde_json::json!({"op": "getFindings"}));
    let findings = findings.as_array().unwrap();
    assert!(
        findings
            .iter()
            .any(|finding| finding["code"] == "while-without-wait"),
        "a loop whose only wait is disabled reports while-without-wait: {findings:?}"
    );
    assert!(
        !findings
            .iter()
            .any(|finding| finding["code"] == "min-wait-loop"),
        "a disabled wait fires no min-wait-loop: {findings:?}"
    );
}

#[test]
fn disabled_rules_and_statements_contribute_no_persistent_objects() {
    let mut program = Program::new();
    let mut off = Rule::new("off", Event::Global).action(Action::call("createHudText", []));
    off.disabled = true;
    program.rule(off);
    program.rule(
        Rule::new("on", Event::Global)
            .action(Action::Disabled {
                action: Box::new(Action::call("createHudText", [])),
            })
            .action(Action::call("createEffect", []))
            .action(Action::Disabled {
                action: Box::new(Action::call("destroyEffect", [])),
            }),
    );
    let service = service_for(program);
    let objects = query(&service, serde_json::json!({"op": "getPersistentObjects"}));
    let objects = objects.as_array().unwrap();
    assert_eq!(
        objects.len(),
        1,
        "only the live createEffect reports: {objects:?}"
    );
    assert_eq!(objects[0]["kind"], "effect");
    assert_eq!(objects[0]["rule"], 1);
    assert_eq!(
        objects[0]["sameKindCleanupInRule"], false,
        "a disabled cleanup call does not count: {objects:?}"
    );
}

#[test]
fn a_subroutine_named_like_a_builtin_neither_edges_nor_counter_writes() {
    let source = r#"
variables {
    global:
        0: index
}
subroutines {
    0: bigMessage
}
rule ("counter loop") {
    event {
        Ongoing - Global;
    }
    actions {
        While(Compare(Global.index, <, 3));
            Modify Global Variable(index, Add, 1);
            Big Message(All Players(All Teams), Custom String("x"));
        End;
    }
}
"#;
    let service = workshop_service_from_text(source);
    let findings = query(&service, serde_json::json!({"op": "getFindings"}));
    let finding = findings
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["code"] == "while-without-wait")
        .expect("a wait-less counter loop reports while-without-wait");
    assert_eq!(
        finding["boundedness"], "statically-bounded",
        "the builtin call writes no counter: {finding:?}"
    );

    let rule = id_for(
        &service,
        serde_json::json!({"op": "listRules"}),
        "counter loop",
    );
    let cfg = query(&service, serde_json::json!({"op": "getCfg", "rule": rule}));
    let blocks = cfg["blocks"].as_array().unwrap();
    assert!(
        !blocks.iter().any(|block| block["calls"]
            .as_array()
            .is_some_and(|calls| calls.contains(&serde_json::json!(0)))),
        "no calls edge reaches the builtin-named subroutine: {blocks:?}"
    );
}

#[test]
fn start_rule_resolves_as_a_subroutine_call() {
    let source = r#"
variables {
    global:
        0: index
}
subroutines {
    0: reset
}
rule ("start rule loop") {
    event {
        Ongoing - Global;
    }
    actions {
        While(Compare(Global.index, <, 3));
            Modify Global Variable(index, Add, 1);
            Start Rule(reset, Restart Rule);
        End;
    }
}
"#;
    let service = workshop_service_from_text(source);
    let findings = query(&service, serde_json::json!({"op": "getFindings"}));
    let finding = findings
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["code"] == "while-without-wait")
        .expect("a wait-less counter loop reports while-without-wait");
    assert_eq!(
        finding["boundedness"], "unknown",
        "Start Rule counts as a possible write like Call Subroutine: {finding:?}"
    );

    let rule = id_for(
        &service,
        serde_json::json!({"op": "listRules"}),
        "start rule loop",
    );
    let cfg = query(&service, serde_json::json!({"op": "getCfg", "rule": rule}));
    let blocks = cfg["blocks"].as_array().unwrap();
    assert!(
        blocks.iter().any(|block| block["calls"]
            .as_array()
            .is_some_and(|calls| calls.contains(&serde_json::json!(0)))),
        "Start Rule produces a calls edge to the subroutine: {blocks:?}"
    );
}

#[test]
fn for_player_variable_bodies_see_the_same_loop_findings() {
    let source = r#"
variables {
    global:
        0: index
    player:
        0: progress
}
rule ("global for") {
    event {
        Ongoing - Global;
    }
    actions {
        For Global Variable(index, 0, 3, 1);
            Wait(0.016, Ignore Condition);
        End;
    }
}
rule ("player for") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    actions {
        For Player Variable(Event Player, progress, 0, 3, 1);
            Wait(0.016, Ignore Condition);
        End;
    }
}
"#;
    let service = workshop_service_from_text(source);
    let findings = query(&service, serde_json::json!({"op": "getFindings"}));
    let min_waits: Vec<_> = findings
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["code"] == "min-wait-loop")
        .collect();
    assert_eq!(
        min_waits.len(),
        2,
        "both For loops report min-wait-loop: {findings:?}"
    );
    assert!(
        min_waits.iter().any(|finding| finding["rule"] == 1),
        "the For Player Variable loop reports too: {min_waits:?}"
    );
}

#[test]
fn while_without_wait_severity_override_applies_to_each_boundedness_class() {
    let source = r#"
variables {
    global:
        0: index
}
rule ("severity override") {
    event {
        Ongoing - Global;
    }
    actions {
        While(True);
            Modify Global Variable(index, Add, 1);
        End;
        While(Compare(Global.index, <, 3));
            Modify Global Variable(index, Add, 1);
        End;
    }
}
"#;
    let catalog = workshop_rs::catalog::Catalog::builtin_ref().unwrap();
    let program = parser::parse_with_context(source, catalog, &Locale::new("en-US"), catalog)
        .expect("Workshop source parses");
    let mut config = LintConfig::default();
    config.set_severity("while-without-wait", Severity::Error);

    let findings: Vec<_> = analyze(&program, &config)
        .into_iter()
        .filter(|finding| finding.code == "while-without-wait")
        .collect();
    assert_eq!(findings.len(), 2);
    assert!(
        findings
            .iter()
            .all(|finding| finding.severity == Severity::Error)
    );
}

#[test]
fn disabled_conditions_are_excluded_from_ongoing_gate_counts() {
    let mut program = Program::new();
    program.rule(
        Rule::new("disabled gates", Event::Global)
            .condition(Condition::disabled(WorkshopValue::call("distance", [])))
            .condition(WorkshopValue::Bool(false))
            .condition(WorkshopValue::call("distance", []))
            .condition(Condition::disabled(WorkshopValue::call("distance", [])))
            .condition(WorkshopValue::call("distance", [])),
    );
    let service = service_for(program);
    let findings = query(&service, serde_json::json!({"op": "getFindings"}));
    let findings = findings.as_array().unwrap();
    assert_eq!(findings.len(), 2);
    assert!(
        findings[0]["message"]
            .as_str()
            .unwrap()
            .contains("condition 2 of 3")
    );
    assert!(
        findings[1]["message"]
            .as_str()
            .unwrap()
            .contains("condition 3 of 3")
    );
}

const NESTED_BINDERS: &str = r#"
variables {
    global:
        0: pool
        1: flag
}
rule ("nested binders") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(pool, Filtered Array(All Players(Team 1), Is True For All(Array(Is Alive(Current Array Element), Compare(Hero Of(Current Array Element), ==, Hero(Ana))), Current Array Element)));
        Set Global Variable(flag, Filtered Array(All Players(Team 1), Compare(Filtered Array(All Players(Team 2), Is Alive(Current Array Element)), !=, Empty Array)));
        Set Global Variable(pool, Filtered Array(Filtered Array(All Players(Team 1), Is Alive(Current Array Element)), Compare(Current Array Element, !=, Null)));
        Set Global Variable(flag, Is True For All(Array(Is Alive(Event Player), Compare(Hero Of(Event Player), ==, Hero(Ana))), Current Array Element));
    }
}
"#;

#[test]
fn binder_nested_in_binder_lambda_reports_the_inner_call() {
    let service = workshop_service_from_text(NESTED_BINDERS);
    let findings = query(&service, serde_json::json!({"op": "getFindings"}));
    let findings = findings.as_array().unwrap();
    let nested: Vec<_> = findings
        .iter()
        .filter(|finding| finding["code"] == "nested-binder-lambda")
        .collect();
    assert_eq!(
        nested.len(),
        2,
        "only the binder calls inside a binder's per-element argument report: {findings:?}"
    );
    for finding in &nested {
        assert_eq!(finding["severity"], "warning");
        assert_eq!(finding["evidence"], "static-indicator");
    }
    let message = nested[0]["message"].as_str().unwrap();
    assert!(
        message.contains("isTrueForAll") && message.contains("filteredArray"),
        "the finding names the inner and enclosing binders: {message}"
    );
    assert_eq!(nested[0]["action"], 0);
    let covered = span_text(NESTED_BINDERS, &nested[0]["span"]);
    assert!(
        covered.contains("Is True For All"),
        "the span locates the inner call: {covered}"
    );
    let message = nested[1]["message"].as_str().unwrap();
    assert!(
        message.contains("filteredArray"),
        "a binder inside the lambda of a non-condition argument also reports: {message}"
    );
    assert_eq!(nested[1]["action"], 1);
}

#[test]
fn binder_nested_in_a_rule_condition_reports() {
    let source = r#"
variables {
    global:
        0: pool
}
rule ("condition nesting") {
    event {
        Ongoing - Global;
    }
    conditions {
        Compare(Count Of(Filtered Array(All Players(Team 1), Is True For Any(Array(Is Alive(Current Array Element)), Current Array Element))), >, 0);
        disabled Compare(Count Of(Filtered Array(All Players(Team 1), Is True For Any(Array(True), Current Array Element))), >, 0);
    }
    actions {
        disabled Set Global Variable(pool, Filtered Array(All Players(Team 1), Is True For All(Array(True), Current Array Element)));
    }
}
"#;
    let service = workshop_service_from_text(source);
    let findings = query(&service, serde_json::json!({"op": "getFindings"}));
    let findings = findings.as_array().unwrap();
    let nested: Vec<_> = findings
        .iter()
        .filter(|finding| finding["code"] == "nested-binder-lambda")
        .collect();
    assert_eq!(
        nested.len(),
        1,
        "the live condition reports; the disabled condition and action do not: {findings:?}"
    );
    assert!(nested[0]["action"].is_null());
}

#[test]
fn nested_binder_lambda_respects_disable() {
    let catalog = workshop_rs::catalog::Catalog::builtin_ref().unwrap();
    let program =
        parser::parse_with_context(NESTED_BINDERS, catalog, &Locale::new("en-US"), catalog)
            .expect("Workshop source parses");
    let mut config = LintConfig::default();
    config.disable("nested-binder-lambda");
    let findings = analyze(&program, &config);
    assert!(
        findings
            .iter()
            .all(|finding| finding.code != "nested-binder-lambda"),
        "a disabled rule reports nothing: {findings:?}"
    );
}

#[test]
fn workshop_cfg_preserves_branches_and_actions_around_loops() {
    let source = r#"
variables {
    global:
        0: index
}
subroutines {
    0: helper
}
rule ("mixed control flow") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(index, 0);
        If(Compare(Global.index, ==, 0));
            Set Global Variable(index, 1);
        Else;
            Set Global Variable(index, 2);
        End;
        While(Compare(Global.index, <, 3));
            Modify Global Variable(index, Add, 1);
            Wait(0.016, Ignore Condition);
        End;
        Call Subroutine(helper);
        Set Global Variable(index, 0);
    }
}
"#;
    let service = workshop_service_from_text(source);
    let rule = id_for(
        &service,
        serde_json::json!({"op": "listRules"}),
        "mixed control flow",
    );
    let cfg = query(&service, serde_json::json!({"op": "getCfg", "rule": rule}));
    let blocks = cfg["blocks"].as_array().unwrap();
    assert!(blocks.iter().any(|block| block["kind"] == "if"));
    assert!(blocks.iter().any(|block| block["kind"] == "while"));
    for action in [0, 1, 2, 4, 6, 7, 8, 10, 11] {
        assert!(
            blocks.iter().any(|block| {
                block["actions"]
                    .as_array()
                    .is_some_and(|actions| actions.contains(&serde_json::json!(action)))
            }),
            "CFG must retain action {action}: {blocks:?}"
        );
    }
    assert!(blocks.iter().any(|block| block["waits"] == true));
    assert!(blocks.iter().any(|block| {
        block["calls"]
            .as_array()
            .is_some_and(|calls| calls.contains(&serde_json::json!(0)))
    }));
    assert!(blocks.iter().any(|block| {
        block["successors"].as_array().is_some_and(|edges| {
            edges.iter().any(|edge| edge["kind"] == "true")
                && edges.iter().any(|edge| edge["kind"] == "false")
        })
    }));
}

#[test]
fn persistent_object_identity_reflects_the_immediate_assignment() {
    let mut program = Program::new();
    program.global_variable(Variable::new("effectId"));
    program.rule(
        Rule::new("retained", Event::Global)
            .action(Action::call("createEffect", []))
            .action(Action::SetGlobalVariable {
                variable: "effectId".into(),
                value: WorkshopValue::call("lastCreatedEntity", []),
            }),
    );
    program.rule(Rule::new("unretained", Event::Global).action(Action::call("createEffect", [])));
    program.rule(
        Rule::new("delayed", Event::Global)
            .action(Action::call("createEffect", []))
            .action(Action::call("print", []))
            .action(Action::SetGlobalVariable {
                variable: "effectId".into(),
                value: WorkshopValue::call("lastCreatedEntity", []),
            }),
    );
    program.rule(
        Rule::new("wrong identity", Event::Global)
            .action(Action::call("createEffect", []))
            .action(Action::SetGlobalVariable {
                variable: "effectId".into(),
                value: WorkshopValue::call("lastTextId", []),
            }),
    );
    let service = service_for(program);
    let objects = query(&service, serde_json::json!({"op": "getPersistentObjects"}));
    assert_eq!(objects[0]["identityRetained"], true);
    assert_eq!(objects[1]["identityRetained"], false);
    assert_eq!(objects[2]["identityRetained"], false);
    assert_eq!(objects[3]["identityRetained"], false);
}

#[test]
fn workshop_input_references_preserve_source_spans() {
    let service = workshop_service("synthetic/declarations-rules");
    let symbol = id_for(
        &service,
        serde_json::json!({"op": "listSymbols"}),
        "hasStarted",
    );
    let references = query(
        &service,
        serde_json::json!({"op": "findReferences", "symbol": symbol}),
    );
    let references = references.as_array().unwrap();
    assert!(
        references
            .iter()
            .any(|reference| reference["kind"] == "write" && reference["span"].is_object()),
        "workshop input references must carry usable spans: {references:?}"
    );
}

#[test]
fn workshop_action_argument_reads_keep_their_authored_spans() {
    let source = r#"
variables {
    global:
        0: result
        1: source
    player:
        0: state
}
rule ("argument spans") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(result, Global.source);
        Set Player Variable(Event Player, state, Global.source);
        For Global Variable(result, Global.source, Global.source, Global.source);
            Wait(0.016, Ignore Condition);
        End;
    }
}
"#;
    let service = workshop_service_from_text(source);
    let symbol = id_for(&service, serde_json::json!({"op": "listSymbols"}), "source");
    let references = query(
        &service,
        serde_json::json!({"op": "findReferences", "symbol": symbol}),
    );
    let reads: Vec<_> = references
        .as_array()
        .unwrap()
        .iter()
        .filter(|reference| reference["kind"] == "read")
        .collect();
    let expected: Vec<_> = source
        .lines()
        .enumerate()
        .flat_map(|(line, text)| {
            text.match_indices("Global.source")
                .map(move |(column, _)| (line + 1, column + 1))
        })
        .collect();
    assert_eq!(reads.len(), expected.len());
    let value_ids: Vec<_> = reads
        .iter()
        .map(|reference| reference["value"].as_u64().expect("action value identity"))
        .collect();
    assert_eq!(
        value_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        value_ids.len(),
        "each authored action value has a local semantic identity"
    );
    for (reference, (line, column)) in reads.iter().zip(expected) {
        assert_eq!(reference["span"]["start"]["line"], line);
        assert_eq!(reference["span"]["start"]["col"], column + "Global.".len());
        assert_eq!(span_text(source, &reference["span"]), "source");
    }
}

#[test]
fn workshop_references_slice_to_the_identifier() {
    let source = fixture_text("real-world/overpy-cake");
    let service = workshop_service_from_text(&source);
    for name in ["cakePos", "i2", "candlePos"] {
        let symbol = id_for(&service, serde_json::json!({"op": "listSymbols"}), name);
        let references = query(
            &service,
            serde_json::json!({"op": "findReferences", "symbol": symbol}),
        );
        let references = references.as_array().unwrap();
        let implicit_modify_reads = source
            .match_indices(&format!("Modify Global Variable({name}"))
            .count();
        assert_eq!(
            references.len(),
            source.match_indices(name).count() + implicit_modify_reads,
            "every authored {name} occurrence is one reference, no more and no fewer"
        );
        assert!(
            references
                .iter()
                .any(|reference| reference["kind"] == "declaration"),
            "{name} references include the declaration: {references:?}"
        );
        for reference in references {
            assert_eq!(
                span_text(&source, &reference["span"]),
                name,
                "reference span slices to the {name} identifier: {reference:?}"
            );
        }
    }
}

#[test]
fn workshop_symbols_report_their_declaration_identifier() {
    let source = fixture_text("real-world/overpy-cake");
    let service = workshop_service_from_text(&source);
    let symbols = query(&service, serde_json::json!({"op": "listSymbols"}));
    let symbols = symbols.as_array().unwrap();
    for name in ["cakePos", "i2", "candlePos"] {
        let symbol = symbols
            .iter()
            .find(|symbol| symbol["name"] == name)
            .unwrap_or_else(|| panic!("listSymbols has {name}"));
        assert_eq!(symbol["kind"], "globalVariable");
        assert_eq!(
            span_text(&source, &symbol["span"]),
            name,
            "symbol span slices to the declaration name: {symbol:?}"
        );
    }
}

#[test]
fn workshop_reference_spans_ignore_same_name_strings_and_comments() {
    let source = r#"
variables {
    global:
        0: needle
        1: target
}
rule ("same-name text") {
    event {
        Ongoing - Global;
    }
    actions {
        // needle in a comment is not a reference
        Big Message(All Players(All Teams), Custom String("needle {0}", Global.needle));
        Set Global Variable(target, Global.needle); // needle
    }
}
"#;
    let service = workshop_service_from_text(source);
    let symbol = id_for(&service, serde_json::json!({"op": "listSymbols"}), "needle");
    let references = query(
        &service,
        serde_json::json!({"op": "findReferences", "symbol": symbol}),
    );
    let references = references.as_array().unwrap();
    assert_eq!(references.len(), 3, "{references:?}");
    for reference in references {
        assert_eq!(
            span_text(source, &reference["span"]),
            "needle",
            "reference must slice the real identifier, not the literal: {reference:?}"
        );
    }
    let (string_line_number, string_line) = source
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("Custom String"))
        .map(|(index, line)| (index + 1, line))
        .unwrap();
    let literal_column = string_line.find("needle").unwrap() + 1;
    let read_column = string_line.find("Global.needle").unwrap() + "Global.".len() + 1;
    let nested_read = references
        .iter()
        .find(|reference| {
            reference["kind"] == "read"
                && reference["span"]["start"]["line"] == string_line_number as u64
        })
        .expect("the read nested in Custom String is reported");
    assert_ne!(
        nested_read["span"]["start"]["col"], literal_column,
        "the nested read must not point inside the string literal"
    );
    assert_eq!(nested_read["span"]["start"]["col"], read_column);
}

#[test]
fn workshop_subroutine_references_slice_to_the_identifier() {
    let source = fixture_text("synthetic/declarations-rules");
    let service = workshop_service("synthetic/declarations-rules");
    let symbol = id_for(
        &service,
        serde_json::json!({"op": "listSymbols"}),
        "showStatus",
    );
    let references = query(
        &service,
        serde_json::json!({"op": "findReferences", "symbol": symbol}),
    );
    let references = references.as_array().unwrap();
    for kind in ["declaration", "definition", "call"] {
        assert_eq!(
            references
                .iter()
                .filter(|reference| reference["kind"] == kind)
                .count(),
            1,
            "one {kind} reference for showStatus: {references:?}"
        );
    }
    for reference in references {
        assert_eq!(span_text(&source, &reference["span"]), "showStatus");
    }
}

#[test]
fn workshop_player_variable_reads_slice_to_the_identifier() {
    let source = r#"
variables {
    player:
        0: stamina
}
rule ("player reads") {
    event {
        Ongoing - Each Player;
        All;
        All;
    }
    actions {
        Set Player Variable(Event Player, stamina, Add(Event Player.stamina, 1));
    }
}
"#;
    let service = workshop_service_from_text(source);
    let symbol = id_for(
        &service,
        serde_json::json!({"op": "listSymbols"}),
        "stamina",
    );
    let references = query(
        &service,
        serde_json::json!({"op": "findReferences", "symbol": symbol}),
    );
    let references = references.as_array().unwrap();
    assert_eq!(references.len(), 3, "{references:?}");
    for reference in references {
        assert_eq!(
            span_text(source, &reference["span"]),
            "stamina",
            "reference span slices to the stamina identifier: {reference:?}"
        );
    }
}

#[test]
fn workshop_references_without_provenance_are_unmapped() {
    let mut program = Program::new();
    program.global_variable(Variable::new("unmapped"));
    program.rule(
        Rule::new("built", Event::Global).action(Action::SetGlobalVariable {
            variable: "unmapped".into(),
            value: WorkshopValue::GlobalVariable("unmapped".into()),
        }),
    );
    let service = service_for(program);
    let symbols = query(&service, serde_json::json!({"op": "listSymbols"}));
    let symbol = symbols
        .as_array()
        .unwrap()
        .iter()
        .find(|symbol| symbol["name"] == "unmapped")
        .unwrap();
    assert!(
        symbol["span"].is_null(),
        "no recorded declaration provenance -> unmapped span: {symbol}"
    );
    let references = query(
        &service,
        serde_json::json!({"op": "findReferences", "symbol": symbol["id"].as_u64().unwrap()}),
    );
    let references = references.as_array().unwrap();
    assert_eq!(
        references.len(),
        2,
        "one write and one read: {references:?}"
    );
    for reference in references {
        assert!(
            reference["span"].is_null(),
            "a reference without provenance is unmapped, not the enclosing span: {reference:?}"
        );
    }
}

const DUPLICATE_NAMES: &str = r#"
variables {
    global:
        0: dup
        1: solo
}
rule ("dup") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(dup, 1);
    }
}
rule ("dup") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(dup, 2);
    }
}
"#;

#[test]
fn name_resolution_finds_unique_symbols_and_rules() {
    let service = workshop_service("synthetic/declarations-rules");
    let score = service.resolve_symbol("score").unwrap();
    assert_eq!(score.name, "score");
    assert_eq!(score.id.index(), 0);
    assert_eq!(service.resolve_rule("player starts").unwrap(), 1);
    assert_eq!(
        service.resolve_symbol("player starts").unwrap_err().code,
        "unknown-symbol"
    );
}

#[test]
fn name_resolution_rejects_unknown_and_ambiguous_names() {
    let service = workshop_service_from_text(DUPLICATE_NAMES);
    let error = service.resolve_symbol("nope").unwrap_err();
    assert_eq!(error.code, "unknown-symbol");
    assert!(error.message.contains("nope"));
    let error = service.resolve_rule("nope").unwrap_err();
    assert_eq!(error.code, "unknown-rule");

    let error = service.resolve_rule("solo").unwrap_err();
    assert_eq!(error.code, "unknown-rule");
    assert!(service.resolve_symbol("solo").is_ok());

    assert_eq!(service.resolve_symbol("dup").unwrap().name, "dup");
    let error = service.resolve_rule("dup").unwrap_err();
    assert_eq!(error.code, "ambiguous-rule");
    assert_eq!(error.fields["candidates"][0]["address"], "dup#1");
}
