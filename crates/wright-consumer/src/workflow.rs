use wright_driver::service::{ToolRequest, ToolService};
use wright_driver::{
    CompilerSession, FindingSelection, InputSpec, Profile, SessionConfig, SourceKind,
};

pub fn run_consumer(input: &str) -> Result<(), String> {
    let source = std::fs::read_to_string(input).map_err(|e| e.to_string())?;
    let config = SessionConfig {
        input: InputSpec::Path(input.into()),
        kind: SourceKind::Auto,
        profile: Profile::Compat,
        ..SessionConfig::default()
    };
    let mut session = CompilerSession::new(config).map_err(|e| e.message)?;

    let check = session.check();
    assert!(check.ok, "check passes: {:?}", check.diagnostics);

    let compile = session.compile();
    assert!(compile.ok, "compile passes: {:?}", compile.diagnostics);
    let output = compile.result.output.expect("compiled output");
    assert!(!output.text.is_empty(), "compiled text is non-empty");
    println!(
        "compile: {} bytes emitted, sha256 {}",
        output.text.len(),
        &output.sha256[..16]
    );

    let analyze = session.analyze();
    assert!(analyze.ok, "analyze passes");
    println!(
        "analyze: {} symbols, {} rules",
        analyze.result.facts["symbols"].as_array().unwrap().len(),
        analyze.result.facts["rules"].as_array().unwrap().len()
    );

    let lint = session.lint();
    assert!(lint.ok, "lint passes: {:?}", lint.diagnostics);
    println!(
        "lint: {} finding(s) across {} rule(s)",
        lint.result.findings.as_array().unwrap().len(),
        lint.result.rules.as_array().unwrap().len()
    );

    let mut service = ToolService::new(&mut session).map_err(|e| e.message)?;
    let capabilities = service.handle(&ToolRequest::Capabilities);
    match capabilities {
        wright_driver::service::ToolResponse::Ok { result } => {
            assert_eq!(result["contract"], "wright-result/v1");
            assert_eq!(result["agent_contract"], "wright-agent/v1");
            assert!(result["operations"].as_array().unwrap().len() >= 10);
            println!(
                "service: {} v{}, {} operations",
                result["name"],
                result["version"],
                result["operations"].as_array().unwrap().len()
            );
        }
        wright_driver::service::ToolResponse::Error { error } => {
            panic!("capabilities failed: {error:?}")
        }
    }
    for request in [
        ToolRequest::Project,
        ToolRequest::Rules {
            name: None,
            file: None,
            max: None,
            all: true,
            rule: None,
            snapshot: None,
            content: false,
        },
        ToolRequest::Findings {
            selection: FindingSelection::default(),
            all: true,
        },
        ToolRequest::Lint {
            selection: FindingSelection::default(),
            all: true,
        },
        ToolRequest::LintRules,
        ToolRequest::CostEstimate {
            selection: FindingSelection::default(),
            rule: None,
            all: true,
        },
        ToolRequest::TargetMetadata,
    ] {
        match service.handle(&request) {
            wright_driver::service::ToolResponse::Ok { result } => {
                if matches!(request, ToolRequest::Lint { .. }) {
                    for finding in result["findings"].as_array().unwrap() {
                        assert!(
                            finding.get("evidence").is_some(),
                            "lint findings carry evidence"
                        );
                    }
                }
            }
            wright_driver::service::ToolResponse::Error { error } => {
                panic!("query failed: {error:?}")
            }
        }
    }

    if input.ends_with(".ws") && !source.is_empty() {
        let transaction = wright_driver::edit::EditTransaction {
            edits: vec![wright_driver::edit::SourceEdit {
                edit_kind: "edit".to_string(),
                source: input.to_string(),
                source_identity: wright_driver::input_identity(&source),
                range: wright_driver::edit::EditRange {
                    start_line: 1,
                    start_col: 1,
                    end_line: 1,
                    end_col: 2,
                },
                new_text: source.chars().next().unwrap().to_string(),
            }],
        };
        match service.handle(&ToolRequest::ValidateEdit {
            sources: Some(std::collections::BTreeMap::from([(
                input.to_string(),
                source.clone(),
            )])),
            transaction,
        }) {
            wright_driver::service::ToolResponse::Ok { result } => {
                assert!(result["ok"].as_bool().unwrap_or(false), "edit validates");
                println!("edit: transaction validated and previewed");
            }
            wright_driver::service::ToolResponse::Error { error } => {
                panic!("validateEditTransaction failed: {error:?}")
            }
        }
    }

    println!("consumer: all public-API workflows succeeded");
    Ok(())
}
