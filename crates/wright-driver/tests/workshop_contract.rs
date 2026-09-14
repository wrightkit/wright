//! Cross-validate Wright's Workshop provider against the released owner
//! parser and semantic-completeness contract over the pinned corpus.

use std::path::{Path, PathBuf};

use wright_driver::WorkshopProvider;
use wright_driver::provider::LanguageProvider;
use wright_driver::workshop_provider::{diagnostic_code, status_for_classification};

#[test]
#[ignore = "requires the released workshop-rs corpus checkout"]
fn provider_matches_released_workshop_contract() {
    let root = std::env::var_os("WRIGHTKIT_WORKSHOP_CORPUS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("WRIGHTKIT_WORKSHOP_CORPUS_DIR must be set for this test"));
    assert!(
        root.is_absolute() && root.is_dir(),
        "corpus root: {}",
        root.display()
    );

    let catalog = workshop_rs::catalog::Catalog::builtin().expect("owner catalog loads");
    let provider = WorkshopProvider::new().expect("Wright provider loads");
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

        let mut expected = program
            .semantic_issues(&catalog)
            .into_iter()
            .map(|issue| {
                (
                    diagnostic_code(issue_kind(issue.kind), &issue.name),
                    status_for_classification(issue.classification),
                )
            })
            .collect::<Vec<_>>();
        let mut actual = provider
            .check(&source, &path)
            .unwrap_or_else(|error| panic!("{} provider: {error}", path.display()))
            .into_iter()
            .map(|diagnostic| (diagnostic.code, diagnostic.status))
            .collect::<Vec<_>>();
        expected.sort_by(|left, right| left.0.cmp(&right.0));
        actual.sort_by(|left, right| left.0.cmp(&right.0));
        assert_eq!(
            actual,
            expected,
            "{} owner semantic contract",
            path.display()
        );
    }
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
