use std::path::PathBuf;

use wright_driver::service::{ToolRequest, ToolResponse, ToolService};
use wright_driver::{CompilerSession, InputSpec, SessionConfig, SourceKind};

const FRESHNESS_V1: &str = r#"
variables {
    global:
        0: score
}
rule ("setup") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(score, 1);
    }
}
"#;

const FRESHNESS_V2: &str = r#"
variables {
    global:
        0: points
}
rule ("setup") {
    event {
        Ongoing - Global;
    }
    actions {
        Set Global Variable(points, sqrt(4));
    }
}
rule ("extra") {
    event {
        Ongoing - Global;
    }
    actions {
        While(Compare(Global.points, <, 3));
            Modify Global Variable(points, Add, 1);
            Wait(0.016, Ignore Condition);
        End;
    }
}
"#;

fn dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("service-reload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn compile(service: &mut ToolService<'_>, name: &str) {
    let request = ToolRequest::Compile {
        output: Some(PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name)),
        force: true,
        to: None,
    };
    assert!(matches!(service.handle(&request), ToolResponse::Ok { .. }));
}

#[test]
fn a_change_after_a_repaired_load_failure_counts_one_reload() {
    let dir = dir();
    let input = dir.join("program.ws");
    std::fs::write(&input, FRESHNESS_V1).unwrap();
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();

    std::fs::write(
        &input,
        "variables {\n    global:\n        0: score\n}\nrule (\"setup\") {\n",
    )
    .unwrap();
    assert!(matches!(
        service.handle(&ToolRequest::Project),
        ToolResponse::Error { .. }
    ));
    assert_eq!(service.reload_count(), 0);

    std::fs::write(&input, FRESHNESS_V2).unwrap();
    compile(&mut service, "repair-count-1.ws");
    assert_eq!(service.reload_count(), 1);

    std::fs::write(&input, FRESHNESS_V1).unwrap();
    compile(&mut service, "repair-count-2.ws");
    assert_eq!(service.reload_count(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}
