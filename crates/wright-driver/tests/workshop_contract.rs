use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use workshop_rs::real_projects::REAL_PROJECT_EXPECTATION as WORKSHOP_EXPECTATION;
use workshop_rs::semantic::{IncompletenessKind, ResidualClassification};
use wright_core::provider::{LanguageProvider, Status};
use wright_driver::{WorkshopProvider, workshop_provider};

#[test]
#[ignore = "requires the pinned external Workshop corpus directory"]
fn provider_matches_workshop_contract() {
    let artifact_root = std::env::var_os("WRIGHTKIT_WORKSHOP_CORPUS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("WRIGHTKIT_WORKSHOP_CORPUS_DIR must be set for this test"));
    assert!(
        artifact_root.is_absolute() && artifact_root.is_dir(),
        "WRIGHTKIT_WORKSHOP_CORPUS_DIR must be an absolute directory: {}",
        artifact_root.display()
    );
    let provider = WorkshopProvider::new().expect("provider initializes");

    for case in WORKSHOP_EXPECTATION.cases {
        let path = find_artifact_by_hash(&artifact_root, case.source_sha256, case.id);
        let bytes =
            std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            case.source_sha256,
            "{} source hash",
            case.id
        );
        let source = String::from_utf8(bytes).expect("Workshop source is UTF-8");
        let diagnostics = provider
            .check(&source, &path)
            .unwrap_or_else(|error| panic!("{} provider failure: {error}", case.id));

        let expected = case
            .residuals
            .iter()
            .map(|residual| {
                (
                    workshop_provider::diagnostic_code(kind_code(residual.kind), residual.identity),
                    Some(status_name(status_for(residual.classification))),
                )
            })
            .collect::<BTreeSet<_>>();
        let actual = diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.code.clone(),
                    Some(status_name(diagnostic.status)),
                )
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(actual, expected, "{} provider parity", case.id);
    }
}

fn find_artifact_by_hash(root: &Path, expected_hash: &str, case_id: &str) -> PathBuf {
    let mut matches = Vec::new();
    collect_artifact_matches(root, expected_hash, &mut matches);
    assert_eq!(
        matches.len(),
        1,
        "{case_id} must resolve exactly one artifact by owner-provided SHA-256, found {}",
        matches.len()
    );
    matches.pop().expect("one artifact match")
}

fn collect_artifact_matches(root: &Path, expected_hash: &str, matches: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(root).unwrap_or_else(|error| {
        panic!(
            "cannot read Workshop corpus directory {}: {error}",
            root.display()
        )
    });
    for entry in entries {
        let entry = entry.expect("read Workshop corpus directory entry");
        let path = entry.path();
        let file_type = entry.file_type().expect("inspect Workshop corpus entry");
        if file_type.is_dir() {
            collect_artifact_matches(&path, expected_hash, matches);
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap_or_else(|error| {
            panic!(
                "cannot read Workshop corpus artifact {}: {error}",
                path.display()
            )
        });
        if format!("{:x}", Sha256::digest(&bytes)) == expected_hash {
            matches.push(path);
        }
    }
}

fn kind_code(kind: IncompletenessKind) -> &'static str {
    match kind {
        IncompletenessKind::RawSetting => "raw-setting",
        IncompletenessKind::UnknownAction => "unknown-action",
        IncompletenessKind::UnknownValue => "unknown-value",
        IncompletenessKind::OpaqueAction => "opaque-action",
    }
}

fn status_for(classification: ResidualClassification) -> Status {
    workshop_provider::status_for_classification(classification)
}

fn status_name(status: Status) -> &'static str {
    match status {
        Status::Supported => "supported",
        Status::Partial => "partial",
        Status::Unsupported => "unsupported",
    }
}
