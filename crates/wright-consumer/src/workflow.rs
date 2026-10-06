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
        },
        ToolRequest::Findings(FindingSelection::default()),
        ToolRequest::Lint(FindingSelection::default()),
        ToolRequest::LintRules,
        ToolRequest::CostEstimate(FindingSelection::default()),
        ToolRequest::TargetMetadata,
    ] {
        match service.handle(&request) {
            wright_driver::service::ToolResponse::Ok { result } => {
                if matches!(request, ToolRequest::Lint(_)) {
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

    // Raw Workshop semantic rename (#434): address one declared symbol by
    // name through the service surface, and verify the validated preview
    // rewrites its occurrences.
    if input.ends_with(".ws") {
        let renamed = match service.handle(&ToolRequest::Symbols {
            kind: Some("globalVariable".to_string()),
            file: None,
            max: None,
        }) {
            wright_driver::service::ToolResponse::Ok { result } => result["symbols"]
                .as_array()
                .and_then(|symbols| symbols.first())
                .and_then(|symbol| symbol.get("name"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            wright_driver::service::ToolResponse::Error { error } => {
                panic!("symbols failed: {error:?}")
            }
        };
        match renamed {
            Some(name) => {
                let rename = service.handle(&ToolRequest::SemanticRename {
                    sources: Some(std::collections::BTreeMap::from([(
                        input.to_string(),
                        source.clone(),
                    )])),
                    target: wright_driver::edit::RenameTarget {
                        symbol: Some(wright_driver::service::Address::Name(name)),
                        source: None,
                        line: None,
                        col: None,
                        to: "renamed_by_consumer".to_string(),
                    },
                });
                match rename {
                    wright_driver::service::ToolResponse::Ok { result } => {
                        assert!(result["ok"].as_bool().unwrap_or(false), "rename validates");
                        assert!(
                            result["preview"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .any(|p| p["new_text"]
                                    .as_str()
                                    .unwrap_or_default()
                                    .contains("renamed_by_consumer")),
                            "the preview rewrites the identifier"
                        );
                        println!("edit: semantic rename validated and previewed");
                    }
                    wright_driver::service::ToolResponse::Error { error } => {
                        panic!("rename failed: {error:?}")
                    }
                }
            }
            None => println!("edit: no global variable to rename (skipped)"),
        }
    }

    println!("consumer: all public-API workflows succeeded");
    Ok(())
}
