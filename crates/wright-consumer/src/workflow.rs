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
        ToolRequest::CostEstimate,
        ToolRequest::TargetMetadata,
    ] {
        let response = service.handle(&request);
        match response {
            wright_driver::service::ToolResponse::Ok { .. } => {}
            wright_driver::service::ToolResponse::Error { error } => {
                panic!("query failed: {error:?}");
            }
        }
    }

    // Safe rename: propose, validate through the pipeline, preview.
    if input.ends_with(".opy") {
        if let Some(name) = first_global(&source) {
            let identity = wright_driver::input_identity(&source);
            let rename = wright_driver::edit::rename_symbol(
                &source,
                &wright_driver::edit::RenameRequest {
                    symbol_kind: "globalVariable".to_string(),
                    from: name.to_string(),
                    to: "renamed_by_consumer".to_string(),
                    source_identity: identity,
                },
            )
            .map_err(|error| error.message)?;
            let validation = wright_driver::edit::validate_edit(
                &source,
                &rename,
                &SessionConfig {
                    input: InputSpec::Path(input.into()),
                    ..SessionConfig::default()
                },
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
                    .contains("renamed_by_consumer")
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
            if trimmed.starts_with("globalvar") {
                let rest = trimmed["globalvar".len()..].trim_start();
                Some(
                    rest.split(|c: char| c.is_whitespace() || c == '=')
                        .next()
                        .unwrap_or(""),
                )
            } else {
                None
            }
        })
        .filter(|name| !name.is_empty())
}
