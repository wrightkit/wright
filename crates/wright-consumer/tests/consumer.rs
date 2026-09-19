//! External-consumer validation (#61): the public embedding API works from
//! outside the core crates on representative inputs.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn workshop_fixture(id: &str) -> PathBuf {
    workspace_root()
        .join("tests/fixtures/workshop")
        .join(id)
        .with_extension("ws")
}

#[test]
fn consumer_runs_all_public_api_workflows_on_representative_inputs() {
    for id in [
        "synthetic/basic-rule",
        "synthetic/control-flow",
        "synthetic/declarations-numbers",
    ] {
        let path = workshop_fixture(id);
        wright_consumer::run_consumer(path.to_str().unwrap())
            .unwrap_or_else(|message| panic!("{id}: {message}"));
    }
}

#[test]
fn consumer_accepts_workshop_inputs() {
    let path = workshop_fixture("synthetic/control-flow");
    wright_consumer::run_consumer(path.to_str().unwrap()).expect("workshop consumer works");
}
