use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use workshop_rs_provider::p0::P0_EXPECTATION;
use workshop_rs_provider::semantic::{IncompletenessKind, ResidualClassification};
use wright_core::provider::{LanguageProvider, Status};
use wright_driver::{WorkshopProvider, workshop_provider};

#[test]
#[ignore = "requires the pinned external P0 artifact directory"]
fn provider_matches_workshop_rs_p0_expectations() {
    let artifact_root = std::env::var_os("WRIGHTKIT_P0_ARTIFACT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("WRIGHTKIT_P0_ARTIFACT_DIR must be set for this test"));
    assert!(
        artifact_root.is_absolute() && artifact_root.is_dir(),
        "WRIGHTKIT_P0_ARTIFACT_DIR must be an absolute directory: {}",
        artifact_root.display()
    );
    let provider = WorkshopProvider::new().expect("provider initializes");

    for case in P0_EXPECTATION.cases {
        let file_name = Path::new(case.source_fixture)
            .file_name()
            .expect("owner contract fixture filename");
        let path = artifact_root.join(file_name);
        let bytes =
            std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            case.source_sha256,
            "{} source hash",
            case.id
        );
        let source = String::from_utf8(bytes).expect("P0 source is UTF-8");
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
