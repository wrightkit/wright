//! Run the released Workshop corpus through Wright's public CLI commands and
//! compare the resulting semantic diagnostics with the owner Program API.

use std::path::{Path, PathBuf};
use std::process::Command;

use wright_driver::workshop_provider::{diagnostic_code, status_for_classification};

#[test]
#[ignore = "requires the released workshop-rs corpus checkout"]
fn real_projects_run_check_and_lint_through_wright() {
    let root = std::env::var_os("WRIGHTKIT_WORKSHOP_CORPUS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("WRIGHTKIT_WORKSHOP_CORPUS_DIR must be set for this test"));
    assert!(
        root.is_absolute() && root.is_dir(),
        "corpus root: {}",
        root.display()
    );
    let catalog = workshop_rs::catalog::Catalog::builtin().expect("owner catalog loads");
    let files = corpus_files(&root);
    assert!(
        !files.is_empty(),
        "released corpus must contain source files"
    );

    for path in files {
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let locale = workshop_rs::detect::resolve_locale(&source, &catalog, None)
            .unwrap_or_else(|error| panic!("{} locale: {error}", path.display()));
        let program = workshop_rs::parser::parse_with_context(&source, &catalog, &locale, &catalog)
            .unwrap_or_else(|error| panic!("{} parse: {error}", path.display()));
        program
            .validate()
            .unwrap_or_else(|error| panic!("{} validation: {error}", path.display()));
        let expected = program
            .semantic_issues(&catalog)
            .into_iter()
            .map(|issue| {
                (
                    diagnostic_code(issue_kind(issue.kind), &issue.name),
                    serde_json::to_value(status_for_classification(issue.classification))
                        .expect("status serializes"),
                )
            })
            .collect::<Vec<_>>();

        let (check, check_success) = run_json("check", &path);
        assert_eq!(check["command"], "check");
        assert_eq!(
            check_success,
            expected.is_empty(),
            "{} exit",
            path.display()
        );
        assert_eq!(
            check["ok"],
            expected.is_empty(),
            "{} status",
            path.display()
        );
        let actual = check["diagnostics"]
            .as_array()
            .expect("check diagnostics array")
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic["code"]
                        .as_str()
                        .expect("diagnostic code")
                        .to_string(),
                    diagnostic["status"].clone(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "{} owner diagnostics", path.display());
        for diagnostic in check["diagnostics"].as_array().unwrap() {
            assert_eq!(
                diagnostic["span"]["path"],
                path.to_string_lossy().as_ref(),
                "{} diagnostic attribution",
                path.display()
            );
        }

        let (lint, lint_success) = run_json("lint", &path);
        assert_eq!(lint["command"], "lint");
        assert_eq!(lint["ok"], check["ok"], "{} lint status", path.display());
        assert_eq!(lint_success, check_success, "{} lint exit", path.display());
        assert_eq!(lint["diagnostics"], check["diagnostics"]);
        assert!(
            lint["result"]["program"]["rules"]
                .as_u64()
                .is_some_and(|rules| rules > 0),
            "{} lint reaches canonical Program",
            path.display()
        );
    }
}

fn run_json(command: &str, path: &Path) -> (serde_json::Value, bool) {
    let output = Command::new(env!("CARGO_BIN_EXE_wright"))
        .args([command, "--kind", "workshop"])
        .arg(path)
        .args(["-f", "json"])
        .output()
        .unwrap_or_else(|error| panic!("wright {command}: {error}"));
    let value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "wright {command} invalid JSON ({}): {error}; stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (value, output.status.success())
}

fn issue_kind(kind: workshop_rs::semantic::IncompletenessKind) -> &'static str {
    match kind {
        workshop_rs::semantic::IncompletenessKind::RawSetting => "raw-setting",
        workshop_rs::semantic::IncompletenessKind::UnknownAction => "unknown-action",
        workshop_rs::semantic::IncompletenessKind::UnknownValue => "unknown-value",
        workshop_rs::semantic::IncompletenessKind::OpaqueAction => "opaque-action",
    }
}

fn corpus_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_files(root, &mut files);
    files.sort();
    files
}

fn collect_files(root: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(root)
        .unwrap_or_else(|error| panic!("cannot read corpus directory {}: {error}", root.display()))
    {
        let entry = entry.expect("read corpus directory entry");
        let path = entry.path();
        let kind = entry.file_type().expect("inspect corpus entry");
        if kind.is_dir() {
            collect_files(&path, files);
        } else if kind.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("ow"))
        {
            files.push(path);
        }
    }
}
