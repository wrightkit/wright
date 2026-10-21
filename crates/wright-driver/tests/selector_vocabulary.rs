use std::path::{Path, PathBuf};

use wright_driver::service::{ToolRequest, ToolResponse, ToolService};
use wright_driver::{CompilerSession, InputSpec, SessionConfig, SourceKind};

fn declarations_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests/fixtures/workshop/synthetic/declarations-rules.ws")
}

#[test]
fn a_rules_request_carrying_name_is_malformed() {
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(declarations_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();

    let request: ToolRequest = serde_json::from_value(serde_json::json!({
        "op": "rules",
        "name": "player starts"
    }))
    .unwrap();
    let ToolResponse::Error { error } = service.handle(&request) else {
        panic!("{request:?} must refuse")
    };
    assert_eq!(error.code, "malformed-request");

    let request: ToolRequest = serde_json::from_value(serde_json::json!({
        "op": "rules",
        "rule": "player starts"
    }))
    .unwrap();
    let ToolResponse::Ok { result } = service.handle(&request) else {
        panic!("`rule` still selects the declared rule")
    };
    assert_eq!(result["rules"].as_array().unwrap().len(), 1);
    assert_eq!(result["rules"][0]["name"], "player starts");
}
