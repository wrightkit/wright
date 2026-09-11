//! External-consumer validation (#61): the public embedding API works from
//! outside the core crates on representative inputs.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

#[test]
fn consumer_runs_all_public_api_workflows_on_the_corpus() {
    for id in [
        "synthetic/basic-rule",
        "synthetic/control-flow",
        "synthetic/declarations-numbers",
    ] {
        let oracle = workspace_root()
            .join("compatibility/fixtures")
            .join(id)
            .join("oracle.json");
        let workshop =
            serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(oracle).unwrap())
                .unwrap()["compile"]["workshop"]
                .as_str()
                .unwrap()
                .to_string();
        let path = workspace_root()
            .join("target/issue-155-consumer")
            .join(format!("{id}.ws"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, workshop).unwrap();
        wright_consumer::run_consumer(path.to_str().unwrap())
            .unwrap_or_else(|message| panic!("{id}: {message}"));
    }
}

#[test]
fn consumer_accepts_workshop_inputs() {
    let text = std::fs::read_to_string(
        workspace_root().join("compatibility/fixtures/synthetic/control-flow/oracle.json"),
    )
    .unwrap();
    let text = serde_json::from_str::<serde_json::Value>(&text).unwrap()["compile"]["workshop"]
        .as_str()
        .unwrap()
        .to_string();
    let dir = workspace_root().join("target/issue-155-consumer");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("program.txt");
    std::fs::write(&path, &text).unwrap();
    wright_consumer::run_consumer(path.to_str().unwrap()).expect("workshop consumer works");
    let _ = std::fs::remove_dir_all(&dir);
}
