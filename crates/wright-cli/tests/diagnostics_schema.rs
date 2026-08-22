use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn check_json_matches_schema_and_snapshot() {
    let root = workspace_root();
    let oracle = std::fs::read_to_string(
        root.join("compatibility/fixtures/real-world/overpy-client-to-server/oracle.json"),
    )
    .expect("real fixture oracle");
    let oracle_value = serde_json::from_str::<serde_json::Value>(&oracle).expect("oracle JSON");
    let source = oracle_value["compile"]["workshop"]
        .as_str()
        .expect("real fixture Workshop output");
    let directory = root.join("target/diagnostics-schema-test");
    std::fs::create_dir_all(&directory).expect("test directory");
    let input = directory.join("client-to-server.txt");
    std::fs::write(&input, source).expect("real fixture source");

    let output = Command::new(env!("CARGO_BIN_EXE_wright"))
        .args([
            "check",
            input.to_str().expect("UTF-8 path"),
            "--format",
            "json",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("wright runs");
    let schema: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("schemas/wright-check-v1.schema.json"))
            .expect("committed schema"),
    )
    .expect("schema JSON");
    let value = validate_output(&schema, &output.stdout);
    insta::assert_json_snapshot!(value);

    let status_source = format!(
        "{source}\nrule (\"provider status\") {{ actions {{ Set Global Variable(0, FutureValue()); }} }}"
    );
    let status_input = directory.join("provider-status.txt");
    std::fs::write(&status_input, status_source).expect("status fixture source");
    let status_output = Command::new(env!("CARGO_BIN_EXE_wright"))
        .args([
            "check",
            status_input.to_str().expect("UTF-8 path"),
            "--format",
            "json",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("wright runs provider status fixture");
    let status_value = validate_output(&schema, &status_output.stdout);
    assert_eq!(
        status_value["diagnostics"][0]["status"],
        serde_json::Value::String("unsupported".to_string())
    );
}

fn validate_output(schema: &serde_json::Value, output: &[u8]) -> serde_json::Value {
    let value: serde_json::Value = serde_json::from_slice(output).expect("JSON output");
    let validator = jsonschema::JSONSchema::compile(schema).expect("schema compiles");
    if let Err(errors) = validator.validate(&value) {
        let errors = errors.map(|error| error.to_string()).collect::<Vec<_>>();
        panic!("check output does not validate: {errors:?}");
    }
    value
}
