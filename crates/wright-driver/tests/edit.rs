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
    // Tests within one binary run in parallel threads, so the directory name
    // must never be shared between test calls.
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-edit-txn-{}-{name}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
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
        validation.preview.is_none(),
        "a compile-invalid transaction returns no validated preview"
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

// -- semantic rename (M14 #129) ------------------------------------------------

fn rename_at(
    root: &Path,
    config: &SessionConfig,
    sources: &BTreeMap<String, String>,
    source: &str,
    line: u32,
    col: u32,
    to: &str,
) -> wright_driver::edit::SemanticRename {
    let _ = root;
    wright_driver::edit::semantic_rename(
        config,
        sources,
        &wright_driver::edit::RenameTarget {
            source: source.to_string(),
            line,
            col,
            to: to.to_string(),
        },
    )
}

#[test]
fn semantic_rename_edits_only_the_resolved_identity_in_opy() {
    // Same-spelled unrelated identifiers and string literals stay untouched:
    // only the resolved symbol's declaration/reference occurrences change.
    let root = temp_project(
        "rename-same-spell",
        &[(
            "main.opy",
            "globalvar score = 0\nglobalvar scoreboard = 1\n\nrule \"r\":\n    @Event global\n    score += scoreboard\n    debug(\"score\")\n",
        )],
    );
    let main_path = root.join("main.opy");
    let source = std::fs::read_to_string(&main_path).unwrap();
    let sources = BTreeMap::from([(main_path.to_string_lossy().into_owned(), source)]);
    let config = opy_config(&root, "main.opy");
    // Position on the `score` declaration (line 1, col 11).
    let rename = rename_at(
        &root,
        &config,
        &sources,
        &main_path.to_string_lossy(),
        1,
        11,
        "total",
    );
    assert!(rename.ok, "rename resolves: {:?}", rename.diagnostics);
    let transaction = rename.transaction.unwrap();
    let previews = rename.preview.as_ref().unwrap();
    let new_text = &previews[0].new_text;
    assert!(
        new_text.contains("globalvar total = 0"),
        "declaration renamed"
    );
    assert!(
        new_text.contains("total += scoreboard"),
        "reference renamed, sibling untouched"
    );
    assert!(
        new_text.contains("globalvar scoreboard = 1"),
        "longer identifier untouched"
    );
    assert!(
        new_text.contains("debug(\"score\")"),
        "string literal untouched"
    );
    assert!(
        transaction
            .edits
            .iter()
            .all(|edit| edit.new_text == "total"),
        "every edit is an exact occurrence replacement"
    );
}

#[test]
fn semantic_rename_edits_cross_file_occurrences_in_opy() {
    let root = temp_project(
        "rename-cross-file",
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
    let sources = BTreeMap::from([
        (main_path.to_string_lossy().into_owned(), main),
        (defs_path.to_string_lossy().into_owned(), defs),
    ]);
    let config = opy_config(&root, "main.opy");
    // The position names the reference inside the main file (line 5, the
    // `score += 1` statement); the declaration in defs.opy is a cross-file
    // occurrence of the same identity.
    let rename = rename_at(
        &root,
        &config,
        &sources,
        &main_path.to_string_lossy(),
        5,
        5,
        "total",
    );
    assert!(
        rename.ok,
        "cross-file rename resolves: {:?}",
        rename.diagnostics
    );
    let previews = rename.preview.as_ref().unwrap();
    assert_eq!(previews.len(), 2, "both files edited");
    let defs_preview = previews
        .iter()
        .find(|preview| preview.source.ends_with("defs.opy"))
        .expect("defs.opy preview");
    assert_eq!(defs_preview.new_text, "globalvar total = 0\n");
    let main_preview = previews
        .iter()
        .find(|preview| preview.source.ends_with("main.opy"))
        .unwrap();
    assert!(main_preview.new_text.contains("total += 1"));
}

#[test]
fn semantic_rename_refuses_collisions_and_unresolved_targets() {
    let root = temp_project(
        "rename-refusals",
        &[(
            "main.opy",
            "globalvar score = 0\nglobalvar total = 1\n\nrule \"r\":\n    @Event global\n    score += 1\n",
        )],
    );
    let main_path = root.join("main.opy");
    let source = std::fs::read_to_string(&main_path).unwrap();
    let sources = BTreeMap::from([(main_path.to_string_lossy().into_owned(), source)]);
    let config = opy_config(&root, "main.opy");

    // Collision: `total` is already declared.
    let rename = rename_at(
        &root,
        &config,
        &sources,
        &main_path.to_string_lossy(),
        1,
        11,
        "total",
    );
    assert!(!rename.ok);
    assert_eq!(rename.diagnostics[0].code, "rename-collision");
    assert!(rename.transaction.is_none(), "no partial transaction");

    // Unresolved position (no identifier there).
    let rename = rename_at(
        &root,
        &config,
        &sources,
        &main_path.to_string_lossy(),
        3,
        1,
        "x",
    );
    assert!(!rename.ok);
    assert_eq!(rename.diagnostics[0].code, "rename-unresolved");

    // Empty new name.
    let rename = rename_at(
        &root,
        &config,
        &sources,
        &main_path.to_string_lossy(),
        1,
        11,
        "",
    );
    assert!(!rename.ok);
    assert_eq!(rename.diagnostics[0].code, "rename-invalid-name");
}

#[test]
fn semantic_rename_works_on_an_ostw_project_cross_file() {
    let root = temp_project(
        "rename-ostw",
        &[
            ("ds.toml", "entry_point=\"main.ostw\"\n"),
            (
                "main.ostw",
                "import \"lib.del\";\nrule: \"main\" {}\nglobalvar Number score = 5;\n",
            ),
            (
                "lib.del",
                "globalvar Number count = 0;\nrule: \"lib\" {\n    score = 1;\n}\n",
            ),
        ],
    );
    let main_path = root.join("main.ostw");
    let lib_path = root.join("lib.del");
    let main = std::fs::read_to_string(&main_path).unwrap();
    let lib = std::fs::read_to_string(&lib_path).unwrap();
    let sources = BTreeMap::from([
        (main_path.to_string_lossy().into_owned(), main),
        (lib_path.to_string_lossy().into_owned(), lib),
    ]);
    let config = ostw_config(&root);
    // Rename the `score` globalvar from its reference inside lib.del; the
    // declaration in main.ostw is a cross-file occurrence.
    let rename = rename_at(
        &root,
        &config,
        &sources,
        &lib_path.to_string_lossy(),
        3,
        5,
        "total",
    );
    assert!(
        rename.ok,
        "OSTW semantic rename resolves: {:?}",
        rename.diagnostics
    );
    let previews = rename.preview.as_ref().unwrap();
    assert_eq!(previews.len(), 2, "both project files edited");
    let main_preview = previews
        .iter()
        .find(|preview| preview.source.ends_with("main.ostw"))
        .unwrap();
    assert!(
        main_preview
            .new_text
            .contains("globalvar Number total = 5;"),
        "declaration renamed in main.ostw"
    );
    let lib_preview = previews
        .iter()
        .find(|preview| preview.source.ends_with("lib.del"))
        .unwrap();
    assert!(
        lib_preview.new_text.contains("total = 1;"),
        "reference renamed in lib.del"
    );
}

#[test]
fn semantic_rename_refuses_when_the_edited_ostw_project_breaks() {
    let root = temp_project(
        "rename-ostw-broken",
        &[
            ("ds.toml", "entry_point=\"main.ostw\"\n"),
            (
                "main.ostw",
                "import \"lib.del\";\nrule: \"main\" {}\nglobalvar Number score = 5;\n",
            ),
            (
                "lib.del",
                "globalvar Number count = 0;\nrule: \"lib\" {\n    score = 1;\n}\n",
            ),
        ],
    );
    let main_path = root.join("main.ostw");
    let lib_path = root.join("lib.del");
    let main = std::fs::read_to_string(&main_path).unwrap();
    let lib = std::fs::read_to_string(&lib_path).unwrap();
    let sources = BTreeMap::from([
        (main_path.to_string_lossy().into_owned(), main),
        (lib_path.to_string_lossy().into_owned(), lib),
    ]);
    let config = ostw_config(&root);
    // A new name that breaks the edited project (a number literal) refuses
    // with the compile diagnostic and no transaction.
    let rename = rename_at(
        &root,
        &config,
        &sources,
        &lib_path.to_string_lossy(),
        3,
        5,
        "5total",
    );
    assert!(!rename.ok, "a breaking OSTW rename refuses");
    assert!(
        rename
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ostw-parse-error"
                || diagnostic.code == "ostw-lex-error"
                || diagnostic.code == "lex-error"
                || diagnostic.code == "parse-error"),
        "the refusal carries the compile diagnostic: {:?}",
        rename.diagnostics
    );
    assert!(rename.transaction.is_none(), "no partial transaction");
}

// -- PM blocker regressions (#128 correctness, PR #133) ------------------------

/// A transaction applying one edit at an explicit source/range.
fn single_edit(
    source_key: &str,
    text: &str,
    new_text: &str,
    start: (u32, u32),
    end: (u32, u32),
) -> SourceEdit {
    SourceEdit {
        edit_kind: "test".to_string(),
        source: source_key.to_string(),
        source_identity: wright_driver::input_identity(text),
        range: EditRange {
            start_line: start.0,
            start_col: start.1,
            end_line: end.0,
            end_col: end.1,
        },
        new_text: new_text.to_string(),
    }
}

/// Apply a transaction's edits directly (no project validation), mapping the
/// previews back to source keys by suffix match.
fn applied_text(
    transaction: &EditTransaction,
    sources: &BTreeMap<String, String>,
    key: &str,
) -> String {
    transaction
        .apply(sources)
        .expect("transaction applies")
        .into_iter()
        .find(|preview| preview.source.ends_with(key))
        .map(|preview| preview.new_text)
        .unwrap_or_default()
}

#[test]
fn same_line_edits_apply_against_the_original_snapshot_when_length_changes() {
    // Blocker 1 (PR #133): two non-overlapping edits on one line where the
    // first replacement is longer must both address the original occurrence
    // — a later range may not drift with the earlier replacement.
    let source = "    a = b + c\n";
    let key = "program.opy".to_string();
    let sources = BTreeMap::from([(key.clone(), source.to_string())]);
    // Replace `a` (cols 5-6) with a longer name, and `c` (cols 13-14) later
    // on the same line.
    let transaction = EditTransaction::new(vec![
        single_edit(&key, source, "alpha", (1, 5), (1, 6)),
        single_edit(&key, source, "charlie", (1, 13), (1, 14)),
    ])
    .unwrap();
    let applied = applied_text(&transaction, &sources, "program.opy");
    assert_eq!(
        applied, "    alpha = b + charlie\n",
        "both edits address the original line coordinates"
    );
}

#[test]
fn later_edit_after_a_multiline_replacement_keeps_original_coordinates() {
    // Blocker 1 (PR #133): an earlier edit that removes lines must not shift
    // the coordinates of a later edit in the same source.
    let source = "rule \"r\":\n    score = 1\n    score = 2\n    score = 3\n";
    let key = "program.opy".to_string();
    let sources = BTreeMap::from([(key.clone(), source.to_string())]);
    let transaction = EditTransaction::new(vec![
        // Remove lines 2-3 (a multiline deletion).
        single_edit(&key, source, "", (2, 1), (3, 14)),
        // Replace `score` on the final line (line 4, cols 5-10).
        single_edit(&key, source, "total", (4, 5), (4, 10)),
    ])
    .unwrap();
    let applied = applied_text(&transaction, &sources, "program.opy");
    assert_eq!(
        applied, "rule \"r\":\n\n\n    total = 3\n",
        "the later edit still addresses the original line 4"
    );
}

#[test]
fn semantic_rename_same_line_occurrences_with_longer_and_shorter_names() {
    // Blocker 1 (PR #133): a semantic rename with several occurrences on one
    // line must edit exactly those occurrences whether the new identifier is
    // longer or shorter than the old one.
    let root = temp_project(
        "rename-same-line",
        &[(
            "main.opy",
            "globalvar score = 0\n\nrule \"r\":\n    @Event global\n    debug(score + score)\n",
        )],
    );
    let main_path = root.join("main.opy");
    let source = std::fs::read_to_string(&main_path).unwrap();
    let sources = BTreeMap::from([(main_path.to_string_lossy().into_owned(), source)]);
    let config = opy_config(&root, "main.opy");

    // Longer new name: all three occurrences (declaration + two same-line
    // reads) change.
    let longer = rename_at(
        &root,
        &config,
        &sources,
        &main_path.to_string_lossy(),
        5,
        11,
        "totalScore",
    );
    assert!(
        longer.ok,
        "longer-name rename resolves: {:?}",
        longer.diagnostics
    );
    let preview = longer.preview.as_ref().unwrap();
    assert!(
        preview[0]
            .new_text
            .contains("debug(totalScore + totalScore)"),
        "all same-line occurrences renamed: {}",
        preview[0].new_text
    );
    assert!(
        !preview[0].new_text.contains("score"),
        "no old spelling remains: {}",
        preview[0].new_text
    );

    // Shorter new name: same guarantees.
    let shorter = rename_at(
        &root,
        &config,
        &sources,
        &main_path.to_string_lossy(),
        5,
        11,
        "s",
    );
    assert!(
        shorter.ok,
        "shorter-name rename resolves: {:?}",
        shorter.diagnostics
    );
    let preview = shorter.preview.as_ref().unwrap();
    assert!(
        preview[0].new_text.contains("    debug(s + s)"),
        "all same-line occurrences renamed: {}",
        preview[0].new_text
    );
}

#[test]
fn invalid_columns_are_rejected_not_clamped() {
    // Blocker 2 (PR #133): under the 1-based exact-range contract, a column
    // of 0 or a column beyond the line must refuse, never silently redirect
    // to a different location.
    let source = "globalvar score = 0\n";
    let key = "program.opy".to_string();
    let sources = BTreeMap::from([(key.clone(), source.to_string())]);
    let config = SessionConfig {
        input: InputSpec::Path(PathBuf::from("program.opy")),
        ..SessionConfig::default()
    };

    let col_zero = single_edit(&key, source, "x", (1, 0), (1, 10));
    let validation = validate_transaction(
        &config,
        &sources,
        &EditTransaction::new(vec![col_zero]).unwrap(),
    );
    assert!(!validation.ok);
    assert_eq!(validation.diagnostics[0].code, "edit-invalid-range");
    assert!(
        validation.preview.is_none(),
        "no preview for a malformed range"
    );

    // Column beyond the line end (the line has 19 characters; 21 is past it).
    let past_end = single_edit(&key, source, "x", (1, 21), (1, 22));
    let validation = validate_transaction(
        &config,
        &sources,
        &EditTransaction::new(vec![past_end]).unwrap(),
    );
    assert!(!validation.ok);
    assert_eq!(validation.diagnostics[0].code, "edit-invalid-range");

    // The valid line-end insertion point (char_count + 1) still works: the
    // edit applies (the range is accepted) and the edited project reaches
    // the compiler, which rejects the now-invalid source.
    let line_end = single_edit(&key, source, " X", (1, 20), (1, 20));
    let validation = validate_transaction(
        &config,
        &sources,
        &EditTransaction::new(vec![line_end]).unwrap(),
    );
    assert!(!validation.ok, "the edited project must not compile");
    assert_eq!(
        validation.diagnostics[0].code, "parse-error",
        "a valid-range edit reaches the compiler: {:?}",
        validation.diagnostics
    );
}

#[test]
fn same_position_zero_width_edits_are_refused_as_conflicts() {
    // Blocker 2 (PR #133): two zero-width insertions at the same position are
    // order-dependent, so they refuse deterministically instead of defining
    // an arbitrary insertion order.
    let source = "globalvar score = 0\n";
    let key = "program.opy".to_string();
    let insert = |col: u32, text: &str| single_edit(&key, source, text, (1, col), (1, col));

    let error = EditTransaction::new(vec![insert(10, "X"), insert(10, "Y")]).unwrap_err();
    assert_eq!(error.code, "edit-zero-width-conflict");
    assert!(
        error.message.contains("order-dependent"),
        "the refusal names the cause: {}",
        error.message
    );

    // A zero-width insertion at another edit's start position is likewise
    // order-dependent and refused.
    let error = EditTransaction::new(vec![
        single_edit(&key, source, "X", (1, 10), (1, 15)),
        insert(10, "Y"),
    ])
    .unwrap_err();
    assert_eq!(error.code, "edit-zero-width-conflict");

    // Distinct zero-width positions remain deterministic and allowed.
    let transaction = EditTransaction::new(vec![insert(10, "X"), insert(12, "Y")]).unwrap();
    assert_eq!(transaction.edits.len(), 2);
}

#[test]
fn compile_invalid_transaction_returns_no_validated_preview() {
    // Blocker 3 (PR #133): any failed validation — including a transaction
    // whose application compiles to an invalid project — returns `ok =
    // false` and no validated preview.
    let root = temp_project(
        "compile-invalid",
        &[(
            "main.opy",
            "globalvar score = 0\n\nrule \"r\":\n    @Event global\n    score += 1\n",
        )],
    );
    let main_path = root.join("main.opy");
    let source = std::fs::read_to_string(&main_path).unwrap();
    let key = main_path.to_string_lossy().into_owned();
    let sources = BTreeMap::from([(key.clone(), source.clone())]);
    let config = opy_config(&root, "main.opy");

    // A transaction whose edited text is syntactically invalid.
    let broken = single_edit(&key, &source, "globalvar score = (;\n", (1, 1), (1, 1));
    let validation = validate_transaction(
        &config,
        &sources,
        &EditTransaction::new(vec![broken]).unwrap(),
    );
    assert!(!validation.ok);
    assert!(
        validation.preview.is_none(),
        "a compile-invalid transaction returns no validated preview"
    );

    // A semantic rename whose edited project fails validation likewise
    // returns no preview.
    let broken_source = "globalvar score\n\nrule \"r\":\n    @Event global\n    debug(score)\n    score += 1\n    debug(score\n";
    let sources = BTreeMap::from([(key.clone(), broken_source.to_string())]);
    let rename = rename_at(&root, &config, &sources, &key, 5, 5, "total");
    assert!(!rename.ok, "a rename that breaks the project refuses");
    assert!(
        rename.preview.is_none(),
        "no validated preview for a compile-invalid rename"
    );
    assert!(rename.transaction.is_none());
}
