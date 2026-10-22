use std::path::Path;

use serde_json::Value;
use wright_driver::service::{ToolRequest, ToolResponse, ToolService};
use wright_driver::{CompilerSession, FindingSelection, InputSpec, SessionConfig, SourceKind};

fn result_of(service: &mut ToolService<'_>, request: ToolRequest) -> Value {
    match service.handle(&request) {
        ToolResponse::Ok { result } => result,
        ToolResponse::Error { error } => panic!("{error:?}"),
    }
}

fn rules_content(service: &mut ToolService<'_>) -> Value {
    result_of(
        service,
        ToolRequest::Rules {
            snapshot: None,
            name: None,
            rule: None,
            file: None,
            max: None,
            content: true,
            all: true,
        },
    )
}

fn position(value: &Value) -> (u64, u64) {
    (
        value["line"].as_u64().unwrap(),
        value["col"].as_u64().unwrap(),
    )
}

fn span_text<'a>(source: &'a str, span: &Value) -> &'a str {
    let offset = |position: &Value| {
        let (line, column) = self::position(position);
        let line_start = source
            .split_inclusive('\n')
            .take(line as usize - 1)
            .map(str::len)
            .sum::<usize>();
        line_start
            + source[line_start..]
                .char_indices()
                .nth(column as usize - 1)
                .unwrap()
                .0
    };
    &source[offset(&span["start"])..offset(&span["end"])]
}

#[test]
fn rules_content_spans_cover_nested_statements_without_semicolons() {
    let source = r#"variables {
    global:
        0: result
}
rule ("nested statements") {
    event { Ongoing - Global; }
    conditions {
        Distance Between(Vector(0, 0, 0), Vector(1, 0, 0)) > 2;
    }
    actions {
        Set Global Variable(result, Add(Count Of(Array(1, 2, 3)), 1));
    }
}
"#;
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Text {
            text: source.into(),
            path: None,
        },
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    let rules = rules_content(&mut service);
    let content = &rules["rules"][0]["content"];
    assert_eq!(
        span_text(source, &content["conditions"][0]["span"]),
        "Distance Between(Vector(0, 0, 0), Vector(1, 0, 0)) > 2"
    );
    assert_eq!(
        span_text(source, &content["actions"][0]["span"]),
        "Set Global Variable(result, Add(Count Of(Array(1, 2, 3)), 1))"
    );
}

#[test]
fn bastion_hot_path_findings_fit_inside_condition_spans() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/workshop/real-world/bastion.ow");
    let source = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        format!(
            "{:x}",
            <sha2::Sha256 as sha2::Digest>::digest(source.as_bytes())
        ),
        "44e453ddf7f373be65aea82d019abd45dd60f5ecb57c8d1607d3576a8bc60259"
    );
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    let rules = rules_content(&mut service);
    let findings = result_of(
        &mut service,
        ToolRequest::Findings {
            selection: FindingSelection::default(),
            all: true,
        },
    );
    let hot: Vec<_> = findings["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["code"] == "ongoing-condition-hot-path")
        .collect();
    assert!(!hot.is_empty());
    for finding in hot {
        let rule = rules["rules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|rule| rule["id"] == finding["rule"])
            .unwrap();
        let span = &finding["span"];
        assert!(
            rule["content"]["conditions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|condition| {
                    let enclosing = &condition["span"];
                    enclosing["path"] == span["path"]
                        && position(&enclosing["start"]) <= position(&span["start"])
                        && position(&span["end"]) <= position(&enclosing["end"])
                }),
            "finding outside its conditions: {finding}"
        );
    }
    let reset = rules["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|rule| rule["name"] == "[Bastion] Reset displaced bot")
        .unwrap();
    let condition = &reset["content"]["conditions"][0]["span"];
    assert_eq!(position(&condition["start"]), (6119, 9));
    assert_eq!(position(&condition["end"]), (6119, 119));
    assert!(span_text(&source, condition).starts_with("Distance Between("));
    assert!(span_text(&source, condition).ends_with("> 2"));
}
