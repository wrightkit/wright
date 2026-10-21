//! `editStatements` (ADR-0029, #855): structured, non-mutating edits of a
//! raw Workshop program's `conditions`/`actions` lists, addressed by
//! `rule`+`list`+`index` and carrying `rule-content-v1` statement nodes.
//! The service resolves every address against the pre-edit program,
//! delegates content conversion and source-edit generation to
//! `workshop-rs`, assembles the owner edits into an `EditTransaction`,
//! and validates it through `validateEditTransaction` — nothing is
//! written and the canonical program is never mutated.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use wright_driver::service::{ToolRequest, ToolResponse, ToolService};
use wright_driver::{CompilerSession, InputSpec, SessionConfig, SourceKind, sha256_hex};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn workshop_path() -> PathBuf {
    workspace_root().join("tests/fixtures/workshop/synthetic/control-flow.ws")
}

fn opy_path() -> PathBuf {
    workspace_root().join("tests/fixtures/opy/basic-rule.opy")
}

fn request(edits: Value, input_identity: &str) -> ToolRequest {
    serde_json::from_value(json!({
        "op": "editStatements",
        "inputIdentity": input_identity,
        "edits": edits,
    }))
    .expect("request deserializes")
}

/// `project`'s `inputIdentity` for a fixture: the raw-bytes digest the
/// loaded input reports.
fn identity(path: &Path) -> String {
    sha256_hex(&std::fs::read(path).unwrap())
}

fn service(path: &Path) -> (CompilerSession, String) {
    service_of(path, SourceKind::Workshop)
}

fn service_of(path: &Path, kind: SourceKind) -> (CompilerSession, String) {
    let session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.to_path_buf()),
        kind,
        ..SessionConfig::default()
    })
    .unwrap();
    let identity = identity(path);
    (session, identity)
}

/// A rule-content-v1 `Wait(0.5, Ignore Condition)` action.
fn wait_node() -> Value {
    json!({"call": "wait", "args": [0.5, {"enum": "Wait", "member": "IGNORE_CONDITION"}]})
}

#[test]
fn edit_statements_replace_action_validates_and_does_not_write() {
    let path = workshop_path();
    let original = std::fs::read_to_string(&path).unwrap();
    let (mut session, identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    // "branch" actions[1] is `Set Global Variable(index, 1)`; names need
    // no snapshot.
    let response = service.handle(&request(
        json!([{
            "rule": "branch",
            "list": "actions",
            "index": 1,
            "change": "replace",
            "statement": wait_node(),
        }]),
        &identity,
    ));
    let ToolResponse::Ok { result } = response else {
        panic!("editStatements refused: {response:?}")
    };
    assert_eq!(result["ok"], json!(true), "{result}");
    let edits = result["transaction"]["edits"].as_array().unwrap();
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0]["kind"], json!("editStatements"));
    let preview = result["preview"].as_array().unwrap();
    assert_eq!(preview.len(), 1);
    let new_text = preview[0]["new_text"].as_str().unwrap();
    assert!(new_text.contains("Wait(0.5"), "{new_text}");
    assert!(
        !new_text.contains("Set Global Variable(index, 1)"),
        "{new_text}"
    );
    // Nothing is written and the served program never changes: the file
    // keeps its bytes and the issuing generation does not move.
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    assert_eq!(service.snapshot_generation(), Some(1));
}

#[test]
fn edit_statements_insert_and_delete_actions() {
    let path = workshop_path();
    let (mut session, identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    // Insert before "bounded while" actions[0] (the `While`) and replace
    // "branch" actions[1] with a subroutine call in one request — every
    // address names the pre-edit program.
    let response = service.handle(&request(
        json!([
            {
                "rule": "bounded while",
                "list": "actions",
                "index": 0,
                "change": "insert",
                "statements": [wait_node()],
            },
            {
                "rule": "branch",
                "list": "actions",
                "index": 1,
                "change": "replace",
                "statement": {"call": "callSubroutine", "args": [{"subroutine": "nope"}]},
            },
        ]),
        &identity,
    ));
    let ToolResponse::Ok { result } = response else {
        panic!("editStatements refused: {response:?}")
    };
    // `nope` is undeclared, so the owner refuses before any transaction
    // assembles — the whole request refuses with the real diagnostics
    // rather than a partial edit set.
    assert_eq!(result["ok"], json!(false), "{result}");
    assert!(!result["diagnostics"].as_array().unwrap().is_empty());
    assert_eq!(result.get("transaction"), None, "{result}");
    assert_eq!(result.get("preview"), None);

    // The same insert alone validates and previews.
    let response = service.handle(&request(
        json!([{
            "rule": "bounded while",
            "list": "actions",
            "index": 0,
            "change": "insert",
            "statements": [wait_node()],
        }]),
        &identity,
    ));
    let ToolResponse::Ok { result } = response else {
        panic!("editStatements refused: {response:?}")
    };
    assert_eq!(result["ok"], json!(true), "{result}");
    let new_text = result["preview"][0]["new_text"].as_str().unwrap();
    assert!(new_text.contains("Wait(0.5"), "{new_text}");
    assert!(
        new_text.contains("While(Compare(Global.index, <, 3))"),
        "{new_text}"
    );
}

#[test]
fn edit_statements_delete_action_previews_the_removal() {
    let path = workshop_path();
    let (mut session, identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    // "bounded while" actions[2] is `Wait(0.016, Ignore Condition)`.
    let response = service.handle(&request(
        json!([{
            "rule": "bounded while",
            "list": "actions",
            "index": 2,
            "change": "delete",
        }]),
        &identity,
    ));
    let ToolResponse::Ok { result } = response else {
        panic!("editStatements refused: {response:?}")
    };
    assert_eq!(result["ok"], json!(true), "{result}");
    let new_text = result["preview"][0]["new_text"].as_str().unwrap();
    assert!(!new_text.contains("Wait(0.016"), "{new_text}");
    assert!(
        new_text.contains("Modify Global Variable(index, Add, 1)"),
        "{new_text}"
    );
}

#[test]
fn edit_statements_insert_condition_creates_the_section() {
    let path = workshop_path();
    let (mut session, identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    // Neither fixture rule carries a `conditions` section; inserting one
    // creates the section before `actions` — `workshop-rs` owns the
    // emission.
    let response = service.handle(&request(
        json!([{
            "rule": "branch",
            "list": "conditions",
            "index": 0,
            "change": "insert",
            "statements": [{
                // `Compare(Global.index, ==, 1)`: the canonical
                // rule-content-v1 comparison is a call named by the
                // operator with the two operands.
                "value": {
                    "call": "==",
                    "args": [
                        {"call": "globalVariable", "args": [{"variable": "index"}]},
                        1.0
                    ]
                }
            }],
        }]),
        &identity,
    ));
    let ToolResponse::Ok { result } = response else {
        panic!("editStatements refused: {response:?}")
    };
    assert_eq!(result["ok"], json!(true), "{result}");
    let new_text = result["preview"][0]["new_text"].as_str().unwrap();
    assert!(new_text.contains("conditions"), "{new_text}");
    // Conditions emit as Workshop expressions, not `Compare` calls.
    assert!(new_text.contains("Global.index == 1"), "{new_text}");
}

#[test]
fn edit_statements_round_trips_a_content_node() {
    // ADR-0029's motivation: a node `rules` returned by `content` feeds
    // straight back as an edit payload — the caller never composes
    // Workshop source.
    let path = workshop_path();
    let (mut session, identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    let rules = serde_json::from_value::<ToolRequest>(json!({
        "op": "rules", "content": true, "all": true,
    }))
    .unwrap();
    let ToolResponse::Ok { result } = service.handle(&rules) else {
        panic!("rules refused")
    };
    // `rules` content carries each statement's read-only `span`; the
    // owner refuses output-only keys on the write path, so the caller
    // drops them before reusing the node.
    let mut node = result["rules"][1]["content"]["actions"][1].clone();
    let object = node.as_object_mut().unwrap();
    object.remove("span");
    object.remove("spans");
    let response = service.handle(&request(
        json!([{
            "rule": "branch",
            "list": "actions",
            "index": 1,
            "change": "replace",
            "statement": node,
        }]),
        &identity,
    ));
    let ToolResponse::Ok { result } = response else {
        panic!("editStatements refused: {response:?}")
    };
    assert_eq!(result["ok"], json!(true), "{result}");
    assert!(
        result["preview"][0]["new_text"]
            .as_str()
            .unwrap()
            .contains("Set Global Variable(index, 1)")
    );
}

#[test]
fn edit_statements_numeric_rule_requires_the_issuing_snapshot() {
    let path = workshop_path();
    let (mut session, identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    let generation = service.snapshot_generation().unwrap();
    for snapshot in [None, Some(json!(generation + 9))] {
        let mut value = json!({
            "op": "editStatements",
            "inputIdentity": identity,
            "edits": [{
                "rule": 0,
                "list": "actions",
                "index": 0,
                "change": "delete",
            }],
        });
        if let Some(snapshot) = snapshot {
            value["snapshot"] = snapshot;
        }
        let request = serde_json::from_value(value).unwrap();
        let ToolResponse::Error { error } = service.handle(&request) else {
            panic!("a numeric rule without its generation must refuse")
        };
        assert_eq!(error.code, "stale-id");
    }
    // The current generation resolves: delete "bounded while"
    // actions[2] (its `Wait`) — a valid transaction whose preview lacks
    // the statement.
    let request = serde_json::from_value::<ToolRequest>(json!({
        "op": "editStatements",
        "inputIdentity": identity,
        "snapshot": generation,
        "edits": [{
            "rule": 0,
            "list": "actions",
            "index": 2,
            "change": "delete",
        }],
    }))
    .unwrap();
    let ToolResponse::Ok { result } = service.handle(&request) else {
        panic!("editStatements refused")
    };
    assert_eq!(result["ok"], json!(true), "{result}");
    assert!(
        !result["preview"][0]["new_text"]
            .as_str()
            .unwrap()
            .contains("Wait(0.016")
    );
}

#[test]
fn edit_statements_out_of_range_and_unknown_rules_refuse() {
    let path = workshop_path();
    let (mut session, identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    let generation = service.snapshot_generation().unwrap();
    // A numeric rule past the program is an envelope refusal like
    // `rules`'s out-of-range index.
    let out_of_range = serde_json::from_value::<ToolRequest>(json!({
        "op": "editStatements",
        "inputIdentity": identity,
        "snapshot": generation,
        "edits": [{"rule": 9, "list": "actions", "index": 0, "change": "delete"}],
    }))
    .unwrap();
    let ToolResponse::Error { error } = service.handle(&out_of_range) else {
        panic!("an out-of-range rule must refuse")
    };
    assert_eq!(error.code, "invalid-selection");
    // An unknown declared name is the analyzer's `unknown-rule`.
    let response = service.handle(&request(
        json!([{"rule": "nope", "list": "actions", "index": 0, "change": "delete"}]),
        &identity,
    ));
    let ToolResponse::Error { error } = response else {
        panic!("an unknown rule name must refuse: {response:?}")
    };
    assert_eq!(error.code, "unknown-rule");
}

#[test]
fn edit_statements_refusal_diagnostics() {
    let path = workshop_path();
    let (mut session, identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    let edit = |extra: Value| {
        let mut object = extra;
        object["list"] = json!("actions");
        if object.get("index").is_none() {
            object["index"] = json!(0);
        }
        object
    };
    let cases: Vec<(Vec<Value>, &str)> = vec![
        // A catalog id the owner does not know.
        (
            vec![edit(json!({
                "rule": "branch", "change": "replace",
                "statement": {"call": "noSuchAction", "args": []},
            }))],
            "invalid-statement",
        ),
        // A condition node addressed at `actions` (kind mismatch).
        (
            vec![edit(json!({
                "rule": "branch", "change": "replace",
                "statement": {"value": {"call": "==", "args": [1, 1]}},
            }))],
            "invalid-statement",
        ),
        // An insert with an empty statement list.
        (
            vec![edit(json!({
                "rule": "branch", "change": "insert", "statements": [],
            }))],
            "invalid-statement",
        ),
        // Two edits at the same statement address.
        (
            vec![
                edit(json!({"rule": "branch", "change": "delete"})),
                edit(json!({"rule": "branch", "change": "delete"})),
            ],
            "edit-duplicate-address",
        ),
        // A statement index past the list.
        (
            vec![edit(json!({
                "rule": "branch", "index": 40, "change": "delete",
            }))],
            "invalid-statement-address",
        ),
        // Output-only span keys are refused by the owner.
        (
            vec![edit(json!({
                "rule": "branch", "change": "replace",
                "statement": {"call": "wait", "args": [0.5], "span": {}},
            }))],
            "invalid-statement",
        ),
    ];
    for (edits, code) in cases {
        let response = service.handle(&request(json!(edits), &identity));
        let ToolResponse::Ok { result } = response else {
            panic!("{code}: a payload refusal must not be a service error: {response:?}")
        };
        assert_eq!(result["ok"], json!(false), "{code}: {result}");
        assert_eq!(
            result["diagnostics"][0]["code"],
            json!(code),
            "{code}: {result}"
        );
        assert_eq!(result.get("transaction"), None, "{code}: {result}");
    }
    // An empty `edits` list plans an empty transaction.
    let response = service.handle(&request(json!([]), &identity));
    let ToolResponse::Ok { result } = response else {
        panic!("empty edits must not error: {response:?}")
    };
    assert_eq!(
        result["diagnostics"][0]["code"],
        json!("edit-empty-transaction")
    );
}

#[test]
fn edit_statements_input_identity_is_the_freshness_precondition() {
    let path = workshop_path();
    let (mut session, _identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    let response = service.handle(&request(
        json!([{"rule": "branch", "list": "actions", "index": 0, "change": "delete"}]),
        "stale-identity",
    ));
    let ToolResponse::Ok { result } = response else {
        panic!("stale identity is a payload refusal: {response:?}")
    };
    assert_eq!(result["ok"], json!(false));
    assert_eq!(result["diagnostics"][0]["code"], json!("edit-stale-source"));
}

#[test]
fn edit_statements_addresses_the_supplied_sources() {
    // #472 semantics: `sources` supplies the caller's current text; the
    // program the addresses name is loaded from that text — not the
    // disk-loaded snapshot — and `inputIdentity` preconditions it.
    let path = workshop_path();
    let original = std::fs::read_to_string(&path).unwrap();
    let supplied = original.replacen(
        "Set Global Variable(index, 1);",
        "Wait(0.5, Ignore Condition);",
        1,
    );
    let supplied_identity = wright_driver::input_identity(&supplied);
    let (mut session, _disk_identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    let display = path.to_string_lossy().to_string();
    let request = serde_json::from_value::<ToolRequest>(json!({
        "op": "editStatements",
        "inputIdentity": supplied_identity,
        "sources": {display: supplied},
        "edits": [{
            "rule": "branch",
            "list": "actions",
            "index": 1,
            "change": "delete",
        }],
    }))
    .unwrap();
    let ToolResponse::Ok { result } = service.handle(&request) else {
        panic!("supplied-sources editStatements refused")
    };
    assert_eq!(result["ok"], json!(true), "{result}");
    let preview = result["preview"][0]["new_text"].as_str().unwrap();
    assert!(
        preview.contains("If(Compare(Global.index, ==, 0))"),
        "{preview}"
    );
    assert!(!preview.contains("Wait(0.5"), "{preview}");
    // The disk file is untouched and the served snapshot never moved.
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    assert_eq!(service.snapshot_generation(), Some(1));
}

#[test]
fn edit_statements_opy_input_refuses_at_the_provider_boundary() {
    let path = opy_path();
    let (mut session, _) = service_of(&path, SourceKind::Opy);
    let mut service = ToolService::new(&mut session).unwrap();
    let response = service.handle(&request(
        json!([{"rule": 0, "list": "actions", "index": 0, "change": "delete"}]),
        "irrelevant",
    ));
    let ToolResponse::Ok { result } = response else {
        panic!("OPY refusal rides the payload: {response:?}")
    };
    assert_eq!(result["ok"], json!(false));
    assert_eq!(
        result["diagnostics"][0]["code"],
        json!("edit-requires-provider")
    );
}

#[test]
fn edit_statements_unparseable_result_refuses() {
    // A statement the owner cannot convert or emit — here a subroutine
    // call to a name the program does not declare — refuses with the
    // real Workshop diagnostics rather than reporting a successful plan.
    let path = workshop_path();
    let (mut session, identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    let response = service.handle(&request(
        json!([{
            "rule": "branch",
            "list": "actions",
            "index": 1,
            "change": "replace",
            "statement": {"call": "callSubroutine", "args": [{"subroutine": "nope"}]},
        }]),
        &identity,
    ));
    let ToolResponse::Ok { result } = response else {
        panic!("editStatements refused: {response:?}")
    };
    // `nope` is undeclared: the owner refuses at plan time, so no
    // transaction assembles — real diagnostics, no preview.
    assert_eq!(result["ok"], json!(false), "{result}");
    assert!(!result["diagnostics"].as_array().unwrap().is_empty());
    assert_eq!(result.get("transaction"), None, "{result}");
    assert_eq!(result.get("preview"), None, "{result}");
}

#[test]
fn edit_statements_sources_must_name_the_loaded_input() {
    let path = workshop_path();
    let (mut session, _identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    let other = std::fs::read_to_string(&path).unwrap();
    let request = serde_json::from_value::<ToolRequest>(json!({
        "op": "editStatements",
        "inputIdentity": wright_driver::input_identity(&other),
        "sources": {"elsewhere.ws": other},
        "edits": [{"rule": "branch", "list": "actions", "index": 0, "change": "delete"}],
    }))
    .unwrap();
    let ToolResponse::Ok { result } = service.handle(&request) else {
        panic!("foreign sources are a payload refusal")
    };
    assert_eq!(result["ok"], json!(false));
    assert_eq!(
        result["diagnostics"][0]["code"],
        json!("edit-unknown-source")
    );
}

#[test]
fn edit_statements_structural_insert_reuses_the_flat_index_space() {
    // Structural actions live in the same flat index space `rules`
    // reports: `If`, body, `Else`, `End` are four addressed positions, so
    // one insert can add a whole `else`/`end` group before an index.
    let path = workshop_path();
    let (mut session, identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    let response = service.handle(&request(
        json!([{
            "rule": "branch",
            "list": "actions",
            "index": 2,
            "change": "insert",
            "statements": [
                {"call": "else", "args": []},
                {"call": "setGlobalVariable", "args": [{"variable": "index"}, 2.0]},
            ],
        }]),
        &identity,
    ));
    let ToolResponse::Ok { result } = response else {
        panic!("editStatements refused: {response:?}")
    };
    assert_eq!(result["ok"], json!(true), "{result}");
    let new_text = result["preview"][0]["new_text"].as_str().unwrap();
    assert!(new_text.contains("Else;"), "{new_text}");
    assert!(
        new_text.contains("Set Global Variable(index, 2)"),
        "{new_text}"
    );
}

#[test]
fn edit_statements_named_rules_ignore_snapshot() {
    // #595: `snapshot` scopes numeric ids only — a declared name resolves
    // against the current program whatever token the request carries.
    let path = workshop_path();
    let (mut session, identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    for snapshot in [json!(0), json!(u64::MAX)] {
        let response = service.handle(
            &serde_json::from_value(json!({
                "op": "editStatements",
                "inputIdentity": identity,
                "snapshot": snapshot,
                "edits": [{"rule": "branch", "list": "actions", "index": 1, "change": "delete"}],
            }))
            .unwrap(),
        );
        let ToolResponse::Ok { result } = response else {
            panic!("a named rule ignores snapshot: {response:?}")
        };
        assert_eq!(result["ok"], json!(true), "{result}");
    }
}

/// A Workshop file with two rules sharing a declared name, written to a
/// unique temp path (the fixture set has no duplicate-name program).
fn temp_duplicate_rules() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wright-855-dup-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("dup.ws");
    std::fs::write(
        &path,
        "variables {\n    global:\n        0: dup\n}\n\nrule (\"dup\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(dup, 1);\n    }\n}\nrule (\"dup\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(dup, 2);\n    }\n}\n",
    )
    .unwrap();
    path
}

#[test]
fn edit_statements_ambiguous_rule_names_refuse() {
    // A name shared by two rules resolves to nothing: the analyzer's
    // `ambiguous-rule`, never a guess (#429).
    let path = temp_duplicate_rules();
    let (mut session, identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    let response = service.handle(&request(
        json!([{"rule": "dup", "list": "actions", "index": 0, "change": "delete"}]),
        &identity,
    ));
    let ToolResponse::Error { error } = response else {
        panic!("an ambiguous name must refuse: {response:?}")
    };
    assert_eq!(error.code, "ambiguous-rule");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn edit_statements_emits_in_the_resolved_locale() {
    // The session's resolved locale drives emission (#759): a zh-CN
    // program's statements convert and emit through `workshop-rs` in zh-CN
    // spellings, never en-US.
    let dir = std::env::temp_dir().join(format!("wright-855-zh-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("zh.ws");
    std::fs::write(
        &path,
        "变量\n{\n    全局:\n        0: x\n}\n规则(\"r\")\n{\n    事件\n    {\n        持续 - 全局;\n    }\n    动作\n    {\n        设置全局变量(x, 1);\n    }\n}\n",
    )
    .unwrap();
    let (mut session, identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    assert_eq!(
        service.loaded().unwrap().origin.locale.as_deref(),
        Some("zh-cn")
    );
    let response = service.handle(&request(
        json!([{
            "rule": "r",
            "list": "actions",
            "index": 0,
            "change": "insert",
            "statements": [wait_node()],
        }]),
        &identity,
    ));
    let ToolResponse::Ok { result } = response else {
        panic!("editStatements refused: {response:?}")
    };
    assert_eq!(result["ok"], json!(true), "{result}");
    let new_text = result["preview"][0]["new_text"].as_str().unwrap();
    assert!(new_text.contains("等待(0.5, 无视条件)"), "{new_text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn edit_statements_locale_override_is_the_resolved_locale() {
    // ADR-0029 + #759: a `--locale` session override is the resolved
    // locale — it replaces detection entirely, so zh-CN text under an
    // `en-US` override never parses (detection alone would have
    // succeeded) while the matching `zh-CN` override emits the zh-CN
    // spellings.
    let dir = std::env::temp_dir().join(format!("wright-855-zhov-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("zh.ws");
    std::fs::write(
        &path,
        "变量\n{\n    全局:\n        0: x\n}\n规则(\"r\")\n{\n    事件\n    {\n        持续 - 全局;\n    }\n    动作\n    {\n        设置全局变量(x, 1);\n    }\n}\n",
    )
    .unwrap();
    let identity = identity(&path);
    let edits = json!([{
        "rule": "r",
        "list": "actions",
        "index": 0,
        "change": "insert",
        "statements": [wait_node()],
    }]);
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.clone()),
        kind: SourceKind::Workshop,
        locale: Some("en-US".to_string()),
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    let ToolResponse::Error { .. } = service.handle(&request(edits.clone(), &identity)) else {
        panic!("zh-CN text under an en-US override must not parse")
    };
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.clone()),
        kind: SourceKind::Workshop,
        locale: Some("zh-CN".to_string()),
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    let ToolResponse::Ok { result } = service.handle(&request(edits, &identity)) else {
        panic!("editStatements refused")
    };
    assert_eq!(result["ok"], json!(true), "{result}");
    assert!(
        result["preview"][0]["new_text"]
            .as_str()
            .unwrap()
            .contains("等待(0.5, 无视条件)")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn edit_statements_ostw_input_names_the_owning_implementation() {
    // OSTW ships no provider, so the kind gate's refusal is
    // `source-provider-unavailable` — reported in the payload like every
    // edit refusal, before any snapshot refresh could run.
    let path = workshop_path();
    let (mut session, _) = service_of(&path, SourceKind::Ostw);
    let mut service = ToolService::new(&mut session).unwrap();
    let response = service.handle(&request(
        json!([{"rule": 0, "list": "actions", "index": 0, "change": "delete"}]),
        "irrelevant",
    ));
    let ToolResponse::Ok { result } = response else {
        panic!("OSTW refusal rides the payload: {response:?}")
    };
    assert_eq!(result["ok"], json!(false));
    assert_eq!(
        result["diagnostics"][0]["code"],
        json!("source-provider-unavailable")
    );
}
