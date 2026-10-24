// The editStatements block-balance contract (#962): a rule's `actions`
// flat stream keeps If/Else If/Else/While/For openers, branches, and End
// markers explicit; an edit whose result leaves an opener unclosed, an
// End unmatched, or a branch outside an open If refuses
// `unbalanced-block` — insert, replace, and delete alike — before any
// source is written.
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

fn request(edits: Value, input_identity: &str) -> ToolRequest {
    serde_json::from_value(json!({
        "op": "editStatements",
        "inputIdentity": input_identity,
        "edits": edits,
    }))
    .expect("request deserializes")
}

fn identity(path: &Path) -> String {
    sha256_hex(&std::fs::read(path).unwrap())
}

fn service(path: &Path) -> (CompilerSession, String) {
    let session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.to_path_buf()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let identity = identity(path);
    (session, identity)
}

fn wait_node() -> Value {
    json!({"call": "wait", "args": [0.5, {"enum": "Wait", "member": "IGNORE_CONDITION"}]})
}

#[test]
fn edit_statements_refuses_every_unpaired_block_form() {
    let path = workshop_path();
    let original = std::fs::read_to_string(&path).unwrap();
    let (mut session, identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    // The "branch" rule's actions are [If, Set Global Variable, End];
    // every single-statement form leaves the result unbalanced.
    let forms = [
        json!({"call": "end", "args": []}),
        json!({"call": "else", "args": []}),
        json!({"call": "elseIf", "args": [true]}),
        json!({"call": "if", "args": [true]}),
        json!({"call": "while", "args": [true]}),
        json!({"call": "forGlobalVariable", "args": [{"variable": "index"}, 0, 3, 1]}),
        json!({"call": "forPlayerVariable", "args": [
            {"call": "eventPlayer", "args": []},
            {"variable": "p"}, 0, 3, 1,
        ]}),
    ];
    for statement in forms {
        let call = statement["call"].as_str().unwrap().to_string();
        let response = service.handle(&request(
            json!([{
                "rule": "branch",
                "list": "actions",
                "index": 0,
                "change": "insert",
                "statements": [statement],
            }]),
            &identity,
        ));
        let ToolResponse::Ok { result } = response else {
            panic!("{call}: a payload refusal must not be a service error: {response:?}")
        };
        assert_eq!(result["ok"], json!(false), "{call}: {result}");
        assert_eq!(
            result["diagnostics"][0]["code"],
            json!("unbalanced-block"),
            "{call}: {result}"
        );
        assert_eq!(result.get("transaction"), None, "{call}: {result}");
        assert_eq!(result.get("preview"), None, "{call}: {result}");
    }
    let response = service.handle(&request(
        json!([{
            "rule": "branch",
            "list": "actions",
            "index": 0,
            "change": "insert",
            "statements": [{"call": "end", "args": []}],
        }]),
        &identity,
    ));
    let ToolResponse::Ok { result } = response else {
        panic!("{response:?}")
    };
    let message = result["diagnostics"][0]["message"].as_str().unwrap();
    assert!(message.contains("`End`"), "{message}");
    // An `Else` nested inside "bounded while"'s `While` is still outside
    // every open `If`, and replacing "branch"'s `If` with a plain action
    // strands its `End` — the same refusal.
    for edits in [
        json!([{
            "rule": "bounded while",
            "list": "actions",
            "index": 1,
            "change": "insert",
            "statements": [{"call": "else", "args": []}],
        }]),
        json!([{
            "rule": "branch",
            "list": "actions",
            "index": 0,
            "change": "replace",
            "statement": wait_node(),
        }]),
    ] {
        let response = service.handle(&request(edits, &identity));
        let ToolResponse::Ok { result } = response else {
            panic!("{response:?}")
        };
        assert_eq!(result["ok"], json!(false), "{result}");
        assert_eq!(
            result["diagnostics"][0]["code"],
            json!("unbalanced-block"),
            "{result}"
        );
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    assert_eq!(service.snapshot_generation(), Some(1));
}

#[test]
fn edit_statements_refuses_partial_block_deletes() {
    let path = workshop_path();
    let original = std::fs::read_to_string(&path).unwrap();
    let (mut session, identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    // "branch" actions: index 0 is `If`, index 2 is its `End`.
    for (index, names) in [(2, "`If`"), (0, "`End`")] {
        let response = service.handle(&request(
            json!([{
                "rule": "branch",
                "list": "actions",
                "index": index,
                "change": "delete",
            }]),
            &identity,
        ));
        let ToolResponse::Ok { result } = response else {
            panic!("{response:?}")
        };
        assert_eq!(result["ok"], json!(false), "{result}");
        let diagnostic = &result["diagnostics"][0];
        assert_eq!(diagnostic["code"], json!("unbalanced-block"), "{result}");
        assert!(
            diagnostic["message"].as_str().unwrap().contains(names),
            "{result}"
        );
        assert_eq!(result.get("transaction"), None, "{result}");
    }
    // Deleting the whole block — every statement it carries, in one
    // transaction — is balanced and previews ok.
    let response = service.handle(&request(
        json!(
            [0, 1, 2]
                .iter()
                .map(|index| json!({
                    "rule": "branch",
                    "list": "actions",
                    "index": index,
                    "change": "delete",
                }))
                .collect::<Vec<_>>()
        ),
        &identity,
    ));
    let ToolResponse::Ok { result } = response else {
        panic!("{response:?}")
    };
    assert_eq!(result["ok"], json!(true), "{result}");
    let new_text = result["preview"][0]["new_text"].as_str().unwrap();
    assert!(!new_text.contains("If(Compare"), "{new_text}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    assert_eq!(service.snapshot_generation(), Some(1));
}

#[test]
fn edit_statements_accepts_a_balanced_block_insert() {
    let text = "variables {\n    global:\n        0: index\n}\n\nrule (\"branch\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(index, 1);\n    }\n}\n";
    // An absolute virtual path keeps the display form equal to the edit
    // source the session emits — the same_source check compares them.
    let path = std::env::temp_dir()
        .join(format!("wright-962-block-{}", std::process::id()))
        .join("block.ws");
    let name = path.to_string_lossy().to_string();
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Text {
            text: text.to_string(),
            path: Some(path),
        },
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let identity = sha256_hex(text.as_bytes());
    let mut service = ToolService::new(&mut session).unwrap();
    let response = service.handle(&request(
        json!([{
            "rule": "branch",
            "list": "actions",
            "index": 1,
            "change": "insert",
            "statements": [
                {"call": "if", "args": [{"call": "globalVariable", "args": [{"variable": "index"}]}]},
                {"call": "setGlobalVariable", "args": [{"variable": "index"}, 2.0]},
                {"call": "end", "args": []},
            ],
        }]),
        &identity,
    ));
    let ToolResponse::Ok { result } = response else {
        panic!("{response:?}")
    };
    assert_eq!(result["ok"], json!(true), "{result}");
    let transaction = serde_json::from_value::<wright_driver::edit::EditTransaction>(
        result["transaction"].clone(),
    )
    .unwrap();
    let mut sources = std::collections::BTreeMap::from([(name, text.to_string())]);
    let applied = service.apply_text_edit_transaction(&transaction, &mut sources);
    assert!(applied.ok, "{applied:?}");
    let ToolResponse::Ok { result } =
        service.handle(&serde_json::from_value(json!({"op": "rules", "content": true})).unwrap())
    else {
        panic!("rules refused")
    };
    let actions = result["rules"][0]["content"]["actions"].as_array().unwrap();
    let calls: Vec<&str> = actions
        .iter()
        .map(|action| action["call"].as_str().unwrap())
        .collect();
    assert_eq!(
        calls,
        ["setGlobalVariable", "if", "setGlobalVariable", "end"]
    );
}

#[test]
fn edit_statements_accepts_an_opener_and_its_end_across_edits() {
    let path = workshop_path();
    let original = std::fs::read_to_string(&path).unwrap();
    let (mut session, identity) = service(&path);
    let mut service = ToolService::new(&mut session).unwrap();
    // Neither edit balances alone: `if` at index 0 opens a block and
    // `end` at index 1 closes it, and the balance is judged on the
    // request's combined result.
    let response = service.handle(&request(
        json!([
            {
                "rule": "bounded while",
                "list": "actions",
                "index": 0,
                "change": "insert",
                "statements": [{"call": "if", "args": [true]}],
            },
            {
                "rule": "bounded while",
                "list": "actions",
                "index": 1,
                "change": "insert",
                "statements": [{"call": "end", "args": []}],
            },
        ]),
        &identity,
    ));
    let ToolResponse::Ok { result } = response else {
        panic!("{response:?}")
    };
    assert_eq!(result["ok"], json!(true), "{result}");
    let new_text = result["preview"][0]["new_text"].as_str().unwrap();
    assert!(new_text.contains("If(True)"), "{new_text}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    assert_eq!(service.snapshot_generation(), Some(1));
}
