use wright_driver::service::{ToolRequest, ToolService};
use wright_driver::{CompilerSession, InputSpec, Profile, SessionConfig, SourceKind};

/// Run every public-API workflow over one input (testable in-process).
pub fn run_consumer(input: &str) -> Result<(), String> {
    let source = std::fs::read_to_string(input).map_err(|error| error.to_string())?;
    let config = SessionConfig {
        input: InputSpec::Path(input.into()),
        kind: SourceKind::Auto,
        profile: Profile::Compat,
        ..SessionConfig::default()
    };
    let mut session = CompilerSession::new(config).map_err(|error| error.message)?;

    // Check through the shared session.
    let check = session.check();
    assert!(check.ok, "check passes: {:?}", check.diagnostics);

    // Compile through the shared session.
    let compile = session.compile();
    assert!(compile.ok, "compile passes: {:?}", compile.diagnostics);
    let output = compile.result.output.expect("compiled output");
    assert!(!output.text.is_empty(), "compiled text is non-empty");
    println!(
        "compile: {} bytes emitted, sha256 {}",
        output.text.len(),
        &output.sha256[..16]
    );

    // Analyze through the shared session.
    let analyze = session.analyze();
    assert!(analyze.ok, "analyze passes");
    println!(
        "analyze: {} findings",
        analyze.result.findings.as_array().unwrap().len()
    );

    // Lint through the shared session (#98): the same pipeline with
    // rule metadata, effective configuration, and evidence-tagged findings.
    let lint = session.lint();
    assert!(lint.ok, "lint passes: {:?}", lint.diagnostics);
    println!(
        "lint: {} finding(s) across {} rule(s)",
        lint.result.findings.as_array().unwrap().len(),
        lint.result.rules.as_array().unwrap().len()
    );

    // Session-aware tool service queries (structured owned results).
    let service = ToolService::new(&mut session).map_err(|error| error.message)?;
    let capabilities = service.handle(&ToolRequest::Capabilities);
    match capabilities {
        wright_driver::service::ToolResponse::Ok { result } => {
            assert_eq!(result["contract"], "wright-result/v1");
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
        ToolRequest::Rules,
        ToolRequest::Findings,
        ToolRequest::Lint,
        ToolRequest::LintRules,
        ToolRequest::CostEstimate,
        ToolRequest::TargetMetadata,
    ] {
        let response = service.handle(&request);
        match response {
            wright_driver::service::ToolResponse::Ok { result } => {
                // The tool lint path carries the same evidence-tagged
                // findings as the session/CLI path.
                if matches!(request, ToolRequest::Lint) {
                    let findings = result["findings"].as_array().unwrap();
                    for finding in findings {
                        assert!(
                            finding.get("evidence").is_some(),
                            "lint findings carry evidence"
                        );
                    }
                }
            }
            wright_driver::service::ToolResponse::Error { error } => {
                panic!("query failed: {error:?}");
            }
        }
    }

    // Safe rename: propose, validate through the project transaction
    // contract, preview (#128: the shared frontend-neutral contract).
    if input.ends_with(".opy") {
        if let Some(name) = first_global(&source) {
            let identity = wright_driver::input_identity(&source);
            let rename = wright_driver::edit::rename_symbol(
                &source,
                &wright_driver::edit::RenameRequest {
                    symbol_kind: "globalVariable".to_string(),
                    from: name.to_string(),
                    to: "renamed_by_consumer".to_string(),
                    source: input.to_string(),
                    source_identity: identity,
                },
            )
            .map_err(|error| error.message)?;
            let sources = std::collections::BTreeMap::from([(input.to_string(), source.clone())]);
            let validation = wright_driver::edit::validate_transaction(
                &SessionConfig {
                    input: InputSpec::Path(input.into()),
                    ..SessionConfig::default()
                },
                &sources,
                &wright_driver::edit::EditTransaction::new(vec![rename])
                    .map_err(|error| error.message)?,
            );
            assert!(
                validation.ok,
                "rename validates: {:?}",
                validation.diagnostics
            );
            assert!(
                validation
                    .preview
                    .as_ref()
                    .unwrap()
                    .iter()
                    .any(|preview| preview.new_text.contains("renamed_by_consumer"))
            );
            println!("edit: safe rename validated and previewed");
        } else {
            println!("edit: no global variable to rename (skipped)");
        }
    }

    println!("consumer: all public-API workflows succeeded");
    Ok(())
}

/// The name of the first global variable declared in the source.
fn first_global(source: &str) -> Option<&str> {
    source
        .lines()
        .find_map(|line| {
            let trimmed = line.trim_start();
            trimmed.strip_prefix("globalvar").map(|rest| {
                rest.trim_start()
                    .split(|c: char| c.is_whitespace() || c == '=')
                    .next()
                    .unwrap_or("")
            })
        })
        .filter(|name| !name.is_empty())
}
