use std::path::PathBuf;

use serde_json::Value;
use wright_driver::service::{ToolRequest, ToolResponse, ToolService};
use wright_driver::{CompilerSession, InputSpec, SessionConfig, SourceKind};

fn result_of(service: &mut ToolService<'_>, request: ToolRequest) -> Value {
    match service.handle(&request) {
        ToolResponse::Ok { result } => result,
        ToolResponse::Error { error } => panic!("{request:?} failed: {error:?}"),
    }
}

fn project_of(text: &str) -> Value {
    let dir = std::env::temp_dir().join(format!(
        "wright-driver-project-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let input: PathBuf = dir.join("program.ws");
    std::fs::write(&input, text).unwrap();
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .expect("session");
    let mut service = ToolService::new(&mut session).expect("service");
    result_of(&mut service, ToolRequest::Project)
}

const PROGRAM: &str = r#"settings
{
    main
    {
        Mode Name: "Synthetic"
        Match Time Limit: 300
    }
}

rule("probe")
{
    event
    {
        Ongoing - Global;
    }
}
"#;

#[test]
fn project_carries_the_settings_block_as_a_read_only_tree() {
    let project = project_of(PROGRAM);
    assert_eq!(
        project["settings"],
        serde_json::json!([
            {
                "name": "main",
                "children": [
                    {"name": "modeName", "value": "Synthetic"},
                    {"name": "Match Time Limit", "value": "300"},
                ],
            }
        ]),
        "project settings tree: {project}"
    );
}

#[test]
fn project_without_a_settings_block_reports_null() {
    let project = project_of(
        r#"rule("probe")
{
    event
    {
        Ongoing - Global;
    }
}
"#,
    );
    assert_eq!(project["settings"], Value::Null, "{project}");
}
