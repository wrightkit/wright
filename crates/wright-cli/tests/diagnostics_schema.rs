use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn check_json_matches_schema_and_snapshot() {
    let root = workspace_root();
    let oracle = std::fs::read_to_string(
        root.join("compatibility/fixtures/real-world/overpy-cake/oracle.json"),
    )
    .expect("real fixture oracle");
    let oracle_value = serde_json::from_str::<serde_json::Value>(&oracle).expect("oracle JSON");
    let source = oracle_value["compile"]["workshop"]
        .as_str()
        .expect("real fixture Workshop output");
    let directory = root.join("target/diagnostics-schema-test");
    std::fs::create_dir_all(&directory).expect("test directory");
    let input = directory.join("cake.txt");
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
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON output");
    let schema: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("schemas/wright-check-v1.schema.json"))
            .expect("committed schema"),
    )
    .expect("schema JSON");
    let validator = jsonschema::JSONSchema::compile(&schema).expect("schema compiles");
    if let Err(errors) = validator.validate(&value) {
        let errors = errors.map(|error| error.to_string()).collect::<Vec<_>>();
        panic!("check output does not validate: {errors:?}");
    }
    insta::assert_json_snapshot!(value);
}
