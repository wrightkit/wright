use std::path::PathBuf;

use serde_json::{Value, json};
use wright_driver::service::{ToolRequest, ToolResponse, ToolService};
use wright_driver::{CompilerSession, InputSpec, SessionConfig, SourceKind, sha256_hex};

fn result_of(service: &mut ToolService<'_>, request: &ToolRequest) -> serde_json::Value {
    match service.handle(request) {
        ToolResponse::Ok { result } => result,
        ToolResponse::Error { error } => panic!("{request:?} failed: {error:?}"),
    }
}

fn temp_workshop(name: &str, text: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-driver-895-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn name_addressing_reports_unknown_and_ambiguous_names_as_structured_errors() {
    let source = r#"
variables {
    global:
        0: dup
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
    let path = temp_workshop("dup.ws", source);
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();

    for (request, code) in [
        (
            ToolRequest::References {
                snapshot: None,
                symbol: "nope".into(),
                kind: None,
                rule: None,
                file: None,
                max: None,
                all: true,
            },
            "unknown-symbol",
        ),
        (
            ToolRequest::Usage {
                snapshot: None,
                symbol: "nope".into(),
            },
            "unknown-symbol",
        ),
        (
            ToolRequest::Cfg {
                snapshot: None,
                rule: "nope".into(),
                kind: None,
                max: None,
                all: true,
            },
            "unknown-rule",
        ),
        (
            ToolRequest::Cfg {
                snapshot: None,
                rule: "dup".into(),
                kind: None,
                max: None,
                all: true,
            },
            "ambiguous-rule",
        ),
    ] {
        match service.handle(&request) {
            ToolResponse::Error { error } => {
                assert_eq!(error.code, code, "{request:?}");
                assert!(error.message.contains("dup") || error.message.contains("nope"));
            }
            ToolResponse::Ok { result } => {
                panic!("{request:?} must not return a result: {result}")
            }
        }
    }

    for symbol in ["dup", "globalVariable:dup"] {
        let result = result_of(
            &mut service,
            &ToolRequest::References {
                snapshot: None,
                symbol: symbol.into(),
                kind: None,
                rule: None,
                file: None,
                max: None,
                all: true,
            },
        );
        assert_eq!(result["kind"], "globalVariable", "{symbol}");
        assert_eq!(result["references"].as_array().unwrap().len(), 3);
    }

    for (request, needle) in [
        (
            ToolRequest::Usage {
                snapshot: None,
                symbol: "dupp".into(),
            },
            "unknown symbol 'dupp'; nearest declared: 'dup'",
        ),
        (
            ToolRequest::Cfg {
                snapshot: None,
                rule: "Dup".into(),
                kind: None,
                max: None,
                all: true,
            },
            "unknown rule 'Dup'; nearest declared: 'dup'",
        ),
        (
            ToolRequest::Usage {
                snapshot: None,
                symbol: "nope".into(),
            },
            "the 'symbols' operation lists them",
        ),
        (
            ToolRequest::Cfg {
                snapshot: None,
                rule: "nope".into(),
                kind: None,
                max: None,
                all: true,
            },
            "the 'rules' operation lists them",
        ),
    ] {
        match service.handle(&request) {
            ToolResponse::Error { error } => {
                assert!(error.message.ends_with(needle), "{request:?}: {error:?}");
            }
            ToolResponse::Ok { result } => {
                panic!("{request:?} must not return a result: {result}")
            }
        }
    }
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn refusals_render_locations_and_bounded_declared_names() {
    let path = temp_workshop(
        "dup.ws",
        r#"rule ("tick") {
    event {
        Ongoing - Global;
    }
}
rule ("tick") {
    event {
        Ongoing - Global;
    }
}
"#,
    );
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();

    let ambiguous = service.handle(&ToolRequest::Cfg {
        snapshot: None,
        rule: "tick".into(),
        kind: None,
        max: None,
        all: true,
    });
    let ToolResponse::Error { error } = ambiguous else {
        panic!("the duplicated name must refuse: {ambiguous:?}")
    };
    for candidate in ["tick#1 at dup.ws:", "tick#2 at dup.ws:"] {
        assert!(
            error.message.contains(candidate),
            "the candidate carries its path:line:column: {error:?}"
        );
    }
    assert!(
        !error.message.contains("{file:") && !error.message.contains(" at 0:"),
        "no placeholder or bare file index: {error:?}"
    );
    let candidates = error.fields["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 2);
    for (index, candidate) in candidates.iter().enumerate() {
        assert_eq!(
            candidate["address"].as_str().unwrap(),
            format!("tick#{}", index + 1)
        );
        assert_eq!(candidate["path"].as_str().unwrap(), "dup.ws");
        assert!(candidate["file"].is_null(), "the index is resolved away");
        assert!(candidate["line"].is_u64() && candidate["column"].is_u64());
    }

    let many = temp_workshop(
        "many.ws",
        &(1..=25)
            .map(|index| {
                format!(
                    "rule (\"rule{index:02}\") {{\n    event {{\n        Ongoing - Global;\n    }}\n}}\n"
                )
            })
            .collect::<String>(),
    );
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(many.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    let ToolResponse::Error { error } = service.handle(&ToolRequest::Cfg {
        snapshot: None,
        rule: "nope".into(),
        kind: None,
        max: None,
        all: true,
    }) else {
        panic!("the unknown rule must refuse")
    };
    assert!(
        error.message.contains("declared rule names (25 total)"),
        "{error:?}"
    );
    assert!(error.message.contains("'rule20'"), "{error:?}");
    assert!(!error.message.contains("'rule21'"), "{error:?}");
    assert!(
        error
            .message
            .ends_with("the 'rules' operation lists them all"),
        "the unbounded form is named in the surface's spelling: {error:?}"
    );
    assert_eq!(error.fields["listOp"], "rules");
    assert_eq!(error.fields["listAll"], true);
    assert_eq!(error.fields["declaredTotal"], 25);

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
    let _ = std::fs::remove_dir_all(many.parent().unwrap());
}

#[test]
fn edit_statements_rule_targets_take_the_address_grammar() {
    let path = temp_workshop(
        "edit-dup.ws",
        r#"variables {
    global:
        0: index
}
rule ("tick") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(index, 1);
    }
}
rule ("tick") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(index, 2);
        Set Global Variable(index, 3);
    }
}
"#,
    );
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let identity = sha256_hex(&std::fs::read(&path).unwrap());
    let mut service = ToolService::new(&mut session).unwrap();
    let request = |edits: Value| {
        serde_json::from_value::<ToolRequest>(json!({
            "op": "editStatements",
            "inputIdentity": identity,
            "edits": edits,
        }))
        .unwrap()
    };

    let ambiguous = service.handle(&request(
        json!([{"rule": "tick", "list": "actions", "index": 0, "change": "delete"}]),
    ));
    let ToolResponse::Error { error } = ambiguous else {
        panic!("the duplicated name must refuse: {ambiguous:?}")
    };
    assert_eq!(error.code, "ambiguous-rule");
    for candidate in ["tick#1", "tick#2"] {
        assert!(error.message.contains(candidate), "{error:?}");
    }

    let ToolResponse::Ok { result } = service.handle(&request(
        json!([{"rule": "tick#2", "list": "actions", "index": 1, "change": "delete"}]),
    )) else {
        panic!("tick#2 must resolve its rule")
    };
    assert_eq!(result["ok"], json!(true), "{result}");
    let new_text = result["preview"][0]["new_text"].as_str().unwrap();
    assert!(
        new_text.contains("Set Global Variable(index, 2)"),
        "{new_text}"
    );
    assert!(
        !new_text.contains("Set Global Variable(index, 3)"),
        "{new_text}"
    );

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn placeholder_spellings_inside_user_content_stay_literal() {
    let path = temp_workshop(
        "brace.ws",
        r#"rule ("a{list:rules}b") {
    event {
        Ongoing - Global;
    }
}
rule ("a{list:rules}b") {
    event {
        Ongoing - Global;
    }
}
"#,
    );
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();

    let ToolResponse::Error { error } = service.handle(&ToolRequest::Cfg {
        snapshot: None,
        rule: "a{list:rules}b".into(),
        kind: None,
        max: None,
        all: true,
    }) else {
        panic!("the duplicated name must refuse")
    };
    assert_eq!(error.code, "ambiguous-rule");
    for candidate in ["a{list:rules}b#1", "a{list:rules}b#2"] {
        assert!(
            error.message.contains(candidate),
            "the name prints verbatim with its ordinal: {error:?}"
        );
    }
    assert!(
        !error.message.contains("the 'rules' operation"),
        "the placeholder spelling inside the name did not render: {error:?}"
    );

    result_of(
        &mut service,
        &ToolRequest::Cfg {
            snapshot: None,
            rule: "a{list:rules}b#1".into(),
            kind: None,
            max: None,
            all: true,
        },
    );

    for argument in ["{file:0}", "{list:rules}"] {
        let ToolResponse::Error { error } = service.handle(&ToolRequest::Symbols {
            kind: None,
            file: Some(argument.into()),
            max: None,
            all: true,
        }) else {
            panic!("an unmatched file selector must refuse")
        };
        assert_eq!(error.code, "invalid-selection");
        assert!(
            error.message.contains(&format!("'{argument}'")),
            "the selector argument prints verbatim: {error:?}"
        );
        assert!(
            !error.message.contains("the 'rules' operation"),
            "the placeholder spelling inside the selector did not render: {error:?}"
        );
    }

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
