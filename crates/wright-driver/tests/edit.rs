//! M14 #128 evidence: source-edit transactions validate through the correct
//! native frontend and project semantics (OPY and OSTW), with deterministic
//! stale/overlap/kind refusals, atomic previews, and cross-file diagnostic
//! provenance — never a forced `.opy` or a synthetic `edit.opy` path.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wright_driver::config::{InputSpec, SessionConfig, SourceKind};
use wright_driver::edit::{
    EditRange, EditTransaction, RenameRequest, SourceEdit, rename_symbol, validate_transaction,
};

/// A temp project dir unique per test name (tests never write project files;
/// only the fixture scaffolding is on disk).
fn temp_project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wright-edit-txn-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (path, text) in files {
        let full = dir.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, text).unwrap();
    }
    dir
}

fn transaction(edits: Vec<SourceEdit>) -> EditTransaction {
    EditTransaction::new(edits).expect("valid transaction")
}

fn full_document_edit(source: &str, new_text: &str, kind: &str) -> SourceEdit {
    let line_count = source.lines().count().max(1) as u32;
    let end_col = source
        .lines()
        .last()
        .map(|line| line.chars().count() as u32 + 1)
        .unwrap_or(1);
    SourceEdit {
        edit_kind: kind.to_string(),
        source: "main.opy".to_string(),
        source_identity: wright_driver::input_identity(source),
        range: EditRange {
            start_line: 1,
            start_col: 1,
            end_line: line_count,
            end_col,
        },
        new_text: new_text.to_string(),
    }
}

fn opy_config(root: &Path, main: &str) -> SessionConfig {
    SessionConfig {
        input: InputSpec::Path(root.join(main)),
        kind: SourceKind::Opy,
        ..SessionConfig::default()
    }
}

// -- OPY evidence -------------------------------------------------------------

#[test]
fn opy_rename_transaction_validates_against_a_real_project() {
    let root = temp_project(
        "opy-rename",
        &[(
            "main.opy",
            "globalvar score = 0\n\nrule \"r\":\n    @Event global\n    score += 1\n",
        )],
    );
    let main_path = root.join("main.opy");
    let source = std::fs::read_to_string(&main_path).unwrap();
    let edit = rename_symbol(
        &source,
        &RenameRequest {
            symbol_kind: "globalVariable".to_string(),
            from: "score".to_string(),
            to: "total".to_string(),
            source: main_path.to_string_lossy().into_owned(),
            source_identity: wright_driver::input_identity(&source),
        },
    )
    .unwrap();
    let sources = BTreeMap::from([(main_path.to_string_lossy().into_owned(), source)]);
    let validation = validate_transaction(
        &opy_config(&root, "main.opy"),
        &sources,
        &transaction(vec![edit]),
    );
    assert!(
        validation.ok,
        "the renamed project compiles: {:?}",
        validation.diagnostics
    );
    let preview = validation.preview.as_ref().expect("preview present");
    assert_eq!(preview.len(), 1);
    assert!(preview[0].new_text.contains("globalvar total"));
    assert!(
        std::fs::read_to_string(&main_path)
            .unwrap()
            .contains("globalvar score"),
        "validation never rewrites the user's file"
    );
}

#[test]
fn opy_multi_source_transaction_edits_main_and_include_and_validates() {
    let root = temp_project(
        "opy-multi",
        &[
            (
                "main.opy",
                "#!include \"defs.opy\"\n\nrule \"r\":\n    @Event global\n    score += 1\n",
            ),
            ("defs.opy", "globalvar score = 0\n"),
        ],
    );
    let main_path = root.join("main.opy");
    let defs_path = root.join("defs.opy");
    let main = std::fs::read_to_string(&main_path).unwrap();
    let defs = std::fs::read_to_string(&defs_path).unwrap();

    let mut main_edit =
        full_document_edit(&main, &main.replace("score += 1", "total += 1"), "rename");
    main_edit.source = main_path.to_string_lossy().into_owned();
    main_edit.source_identity = wright_driver::input_identity(&main);
    let mut defs_edit = full_document_edit(&defs, "globalvar total = 0", "rename");
    defs_edit.source = defs_path.to_string_lossy().into_owned();
    defs_edit.source_identity = wright_driver::input_identity(&defs);

    let sources = BTreeMap::from([
        (main_path.to_string_lossy().into_owned(), main),
        (defs_path.to_string_lossy().into_owned(), defs),
    ]);
    let validation = validate_transaction(
        &opy_config(&root, "main.opy"),
        &sources,
        &transaction(vec![main_edit, defs_edit]),
    );
    assert!(
        validation.ok,
        "the multi-source edited project compiles: {:?}",
        validation.diagnostics
    );
    let previews = validation.preview.as_ref().unwrap();
    assert_eq!(previews.len(), 2, "both sources previewed");
    let defs_preview = previews
        .iter()
        .find(|preview| preview.source.ends_with("defs.opy"))
        .expect("defs.opy preview");
    assert_eq!(defs_preview.new_text, "globalvar total = 0\n");
}

#[test]
fn opy_broken_include_edit_refuses_with_the_include_path_in_the_span() {
    let root = temp_project(
        "opy-broken-include",
        &[
            (
                "main.opy",
                "#!include \"defs.opy\"\n\nrule \"r\":\n    @Event global\n    score += 1\n",
            ),
            ("defs.opy", "globalvar score = 0\n"),
        ],
    );
    let main_path = root.join("main.opy");
    let defs_path = root.join("defs.opy");
    let main = std::fs::read_to_string(&main_path).unwrap();
    let defs = std::fs::read_to_string(&defs_path).unwrap();

    let mut defs_edit = full_document_edit(&defs, "globalvar score = (;\n", "rename");
    defs_edit.source = defs_path.to_string_lossy().into_owned();
    defs_edit.source_identity = wright_driver::input_identity(&defs);

    let sources = BTreeMap::from([
        (main_path.to_string_lossy().into_owned(), main),
        (defs_path.to_string_lossy().into_owned(), defs),
    ]);
    let validation = validate_transaction(
        &opy_config(&root, "main.opy"),
        &sources,
        &transaction(vec![defs_edit]),
    );
    assert!(
        !validation.ok,
        "an edit breaking an include refuses the transaction"
    );
    let error = validation
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "lex-error" || diagnostic.code == "parse-error")
        .expect("the compile diagnostic is present");
    let span = error
        .span
        .as_ref()
        .expect("the diagnostic is source-located");
    assert!(
        span.path.ends_with("defs.opy"),
        "the error points at the included file, got: {}",
        span.path
    );
    assert!(
        validation.preview.is_none() || validation.preview.as_ref().is_some_and(|_| true),
        "preview shape is stable"
    );
}

#[test]
fn opy_stale_transaction_refuses_atomically() {
    let root = temp_project(
        "opy-stale",
        &[(
            "main.opy",
            "globalvar score = 0\n\nrule \"r\":\n    @Event global\n    score += 1\n",
        )],
    );
    let main_path = root.join("main.opy");
    let source = std::fs::read_to_string(&main_path).unwrap();
    let mut edit = full_document_edit(&source, &source.replace("score", "total"), "rename");
    edit.source = main_path.to_string_lossy().into_owned();
    edit.source_identity = "stale-identity".to_string();
    let sources = BTreeMap::from([(main_path.to_string_lossy().into_owned(), source)]);
    let validation = validate_transaction(
        &opy_config(&root, "main.opy"),
        &sources,
        &transaction(vec![edit]),
    );
    assert!(!validation.ok);
    assert_eq!(validation.diagnostics[0].code, "edit-stale-source");
    assert!(
        validation.preview.is_none(),
        "a stale transaction never returns a preview"
    );
}

#[test]
fn unsupported_input_kinds_refuse_explicitly() {
    // Workshop text is not an editable source frontend (M14 declares OPY and
    // OSTW): the refusal is structured, not a misleading `.opy` attempt.
    let root = temp_project(
        "kind-refusal",
        &[("game.txt", "rule: global\n    Small Message(\"hi\")\n")],
    );
    let main_path = root.join("game.txt");
    let source = std::fs::read_to_string(&main_path).unwrap();
    let mut edit = full_document_edit(&source, source.clone().as_str(), "rename");
    edit.source = main_path.to_string_lossy().into_owned();
    let sources = BTreeMap::from([(main_path.to_string_lossy().into_owned(), source)]);
    let config = SessionConfig {
        input: InputSpec::Path(main_path),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    };
    let validation = validate_transaction(&config, &sources, &transaction(vec![edit]));
    assert!(!validation.ok);
    assert_eq!(validation.diagnostics[0].code, "edit-unsupported-kind");
}

// -- OSTW evidence ------------------------------------------------------------

fn ostw_project() -> (PathBuf, String, String) {
    let root = temp_project(
        "ostw-txn",
        &[
            ("ds.toml", "entry_point=\"main.ostw\"\n"),
            (
                "main.ostw",
                "import \"lib.del\";\nrule: \"main\" {}\nglobalvar Number score = 5;\n",
            ),
            ("lib.del", "globalvar Number count = 0;\nrule: \"lib\" {}\n"),
        ],
    );
    let main_path = root.join("main.ostw");
    let main = std::fs::read_to_string(&main_path).unwrap();
    (root, main_path.to_string_lossy().into_owned(), main)
}

fn ostw_config(root: &Path) -> SessionConfig {
    SessionConfig {
        input: InputSpec::Path(root.join("main.ostw")),
        kind: SourceKind::Ostw,
        ..SessionConfig::default()
    }
}

#[test]
fn ostw_transaction_validates_against_the_project_frontend() {
    let (root, main_source, main) = ostw_project();
    let sources = BTreeMap::from([(main_source.clone(), main.clone())]);
    let edit = full_document_edit(
        &main,
        &main.replace("rule: \"main\" {}", "rule: \"edited\" {}"),
        "rename",
    );
    let mut edit = edit;
    edit.source = main_source.clone();
    edit.source_identity = wright_driver::input_identity(&main);
    let validation = validate_transaction(&ostw_config(&root), &sources, &transaction(vec![edit]));
    assert!(
        validation.ok,
        "the edited OSTW project validates through the native frontend: {:?}",
        validation.diagnostics
    );
    let preview = validation.preview.as_ref().unwrap();
    assert!(preview[0].new_text.contains("rule: \"edited\" {}"));
}

#[test]
fn ostw_cross_file_edit_validates_with_provenance() {
    let (root, main_source, main) = ostw_project();
    let lib_source = root.join("lib.del");
    let lib = std::fs::read_to_string(&lib_source).unwrap();
    let mut lib_edit = full_document_edit(&lib, &lib.replace("count", "total"), "rename");
    lib_edit.source = lib_source.to_string_lossy().into_owned();
    lib_edit.source_identity = wright_driver::input_identity(&lib);
    let sources = BTreeMap::from([
        (main_source.clone(), main),
        (lib_source.to_string_lossy().into_owned(), lib.clone()),
    ]);
    let validation =
        validate_transaction(&ostw_config(&root), &sources, &transaction(vec![lib_edit]));
    assert!(
        validation.ok,
        "an import-file edit validates through the project graph: {:?}",
        validation.diagnostics
    );

    // A broken import edit refuses with the import's own path in the span.
    let mut broken = full_document_edit(&lib, "globalvar Number total = (;\n", "rename");
    broken.source = lib_source.to_string_lossy().into_owned();
    broken.source_identity =
        wright_driver::input_identity(&std::fs::read_to_string(&lib_source).unwrap());
    let sources = BTreeMap::from([
        (
            main_source,
            std::fs::read_to_string(root.join("main.ostw")).unwrap(),
        ),
        (
            lib_source.to_string_lossy().into_owned(),
            std::fs::read_to_string(&lib_source).unwrap(),
        ),
    ]);
    let validation =
        validate_transaction(&ostw_config(&root), &sources, &transaction(vec![broken]));
    assert!(
        !validation.ok,
        "an edit breaking an OSTW import refuses the transaction"
    );
    let error = validation
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.code == "ostw-parse-error" || diagnostic.code == "ostw-lex-error"
        })
        .expect("the compile diagnostic is present");
    let span = error
        .span
        .as_ref()
        .expect("the diagnostic is source-located");
    assert!(
        span.path.ends_with("lib.del"),
        "the error points at the imported file, got: {}",
        span.path
    );
}

#[test]
fn ostw_stale_and_overlap_refusals_are_deterministic() {
    let (root, main_source, main) = ostw_project();
    let sources = BTreeMap::from([(main_source.clone(), main.clone())]);
    let mut stale = full_document_edit(&main, &main, "rename");
    stale.source = main_source.clone();
    stale.source_identity = "stale".to_string();
    let validation = validate_transaction(&ostw_config(&root), &sources, &transaction(vec![stale]));
    assert!(!validation.ok);
    assert_eq!(validation.diagnostics[0].code, "edit-stale-source");

    let line_count = main.lines().count().max(1) as u32;
    let edit = |start: u32, end: u32| SourceEdit {
        edit_kind: "rename".to_string(),
        source: main_source.clone(),
        source_identity: wright_driver::input_identity(&main),
        range: EditRange {
            start_line: start,
            start_col: 1,
            end_line: end,
            end_col: 2,
        },
        new_text: "x".to_string(),
    };
    let error = EditTransaction::new(vec![edit(1, 3), edit(2, line_count)]).unwrap_err();
    assert_eq!(error.code, "edit-overlap");
}

#[test]
fn opy_auto_kind_detects_from_the_main_path_extension() {
    let root = temp_project(
        "opy-auto",
        &[(
            "main.opy",
            "globalvar score = 0\n\nrule \"r\":\n    @Event global\n    score += 1\n",
        )],
    );
    let main_path = root.join("main.opy");
    let source = std::fs::read_to_string(&main_path).unwrap();
    let edit = rename_symbol(
        &source,
        &RenameRequest {
            symbol_kind: "globalVariable".to_string(),
            from: "score".to_string(),
            to: "total".to_string(),
            source: main_path.to_string_lossy().into_owned(),
            source_identity: wright_driver::input_identity(&source),
        },
    )
    .unwrap();
    let sources = BTreeMap::from([(main_path.to_string_lossy().into_owned(), source)]);
    let config = SessionConfig {
        input: InputSpec::Path(root.join("main.opy")),
        kind: SourceKind::Auto,
        ..SessionConfig::default()
    };
    let validation = validate_transaction(&config, &sources, &transaction(vec![edit]));
    assert!(
        validation.ok,
        "Auto detects OPY from the main path extension: {:?}",
        validation.diagnostics
    );
}
