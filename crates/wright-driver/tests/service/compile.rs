use super::{
    CompilerSession, InputSpec, SessionConfig, SourceKind, ToolRequest, ToolService, result_of,
    workshop_path,
};

#[test]
fn agent_compile_writes_a_file_and_returns_no_program_text() {
    let out = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("agent-compile.ow");
    let _ = std::fs::remove_file(&out);
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(workshop_path()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .unwrap();
    let mut service = ToolService::new(&mut session).unwrap();
    let request = |force| ToolRequest::Compile {
        output: Some(out.clone()),
        force,
        to: None,
    };

    let first = result_of(&mut service, &request(false));
    assert_eq!(first["ok"], true, "{first}");
    let output = &first["result"]["output"];
    assert_eq!(output["written_to"], out.to_str().unwrap());
    assert!(output.get("text").is_none(), "{output}");
    let written = std::fs::read_to_string(&out).unwrap();
    assert_eq!(
        output["sha256"],
        wright_driver::sha256_hex(written.as_bytes())
    );

    let again = result_of(&mut service, &request(false));
    assert_eq!(again["exit"], 2, "{again}");
    assert!(
        again["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| diagnostic["code"] == "output-exists"),
        "{again}"
    );
    assert!(result_of(&mut service, &request(true))["ok"] == true);
    let _ = std::fs::remove_file(&out);
}
