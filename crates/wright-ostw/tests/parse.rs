//! Corpus-driven frontend regressions (#117): the real committed protect-ban
//! closure must load and parse natively, quoted imports resolve with correct
//! multi-file provenance, and deterministic negative fixtures cover malformed
//! syntax, missing imports, invalid entry points, and unsupported ds.toml
//! configuration. Assertions are on observable outcomes, never on hardcoded
//! parse trees.

use std::path::{Path, PathBuf};

use wright_ir::source::FileId;
use wright_ostw::project::{FileRecord, OstwOutcome};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn corpus_root() -> PathBuf {
    workspace_root().join("compatibility/ostw/corpus/protect-ban")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

fn compile_project(root: &Path, main_rel: &str) -> OstwOutcome {
    let main_text = read(&root.join(main_rel));
    wright_ostw::compile(&main_text, Some(main_rel), root)
}

/// The 16 committed protect-ban source files (project-relative).
const PROTECT_BAN_SOURCES: &[&str] = &[
    "Credits.ostw",
    "coreDebug.ostw",
    "interface/ClickArea.del",
    "interface/HeroSelect.del",
    "interface/HeroSelectConfig.del",
    "interface/HeroSelectFunctions.del",
    "interface/MapData.del",
    "interface/PlayerInterface.del",
    "interface/miscSetup.del",
    "main.ostw",
    "protectBanFull.ostw",
    "utils/AltFont.del",
    "utils/Colors.del",
    "utils/Math.del",
    "utils/ScreenToWorld.del",
    "utils/ServerLoad.del",
];

fn sources(outcome: &OstwOutcome) -> Vec<&FileRecord> {
    outcome
        .project
        .as_ref()
        .expect("project loads")
        .files
        .iter()
        .filter(|file| file.source)
        .collect()
}

#[test]
fn entire_protect_ban_closure_loads_and_parses() {
    let outcome = compile_project(&corpus_root(), "main.ostw");
    assert!(
        outcome.error.is_none(),
        "project must load: {:?}",
        outcome.error
    );
    let project = outcome.project.as_ref().expect("project loads");
    assert_eq!(project.entry, "main.ostw", "ds.toml entry_point");

    let files = sources(&outcome);
    assert_eq!(files.len(), PROTECT_BAN_SOURCES.len());
    for source in PROTECT_BAN_SOURCES {
        let record = files
            .iter()
            .find(|file| file.path == *source)
            .unwrap_or_else(|| panic!("missing source {source} in the registry"));
        assert!(record.parsed, "{} must parse cleanly", record.path);
        assert!(record.cst.is_some(), "{} must carry its CST", record.path);
    }

    // No in-closure parse errors.
    let parse_errors: Vec<_> = outcome
        .diagnostics
        .iter()
        .filter(|error| error.code == "ostw-parse-error" || error.code == "ostw-lex-error")
        .collect();
    assert!(
        parse_errors.is_empty(),
        "no in-closure parse errors, got: {:?}",
        parse_errors
    );

    // The only diagnostics are the known out-of-closure missing imports.
    // (HeroSelect.del's `import "../OSTWUtils/Diagnostics.del"` sits inside a
    // block comment and is correctly not an import.)
    let missing: Vec<_> = outcome
        .diagnostics
        .iter()
        .filter(|error| error.code == "ostw-missing-import")
        .collect();
    assert_eq!(
        missing.len(),
        4,
        "protect-ban has 4 out-of-closure imports (customGameSettings.lobby + 3 OSTWUtils): {:?}",
        outcome.diagnostics
    );
    for diagnostic in missing {
        assert!(
            diagnostic.span.is_some(),
            "missing-import diagnostics carry a source location"
        );
    }
}

#[test]
fn quoted_imports_resolve_relative_to_the_importing_file() {
    let outcome = compile_project(&corpus_root(), "main.ostw");
    let project = outcome.project.as_ref().expect("project loads");
    let by_path = |path: &str| {
        project
            .files
            .iter()
            .find(|file| file.path == path)
            .unwrap_or_else(|| panic!("missing file {path}"))
    };

    let main = by_path("main.ostw");
    let credits = by_path("Credits.ostw");
    let hero_select = by_path("interface/HeroSelect.del");
    let misc_setup = by_path("interface/miscSetup.del");
    assert_eq!(
        main.imports
            .iter()
            .map(|import| import.target)
            .collect::<Vec<_>>(),
        vec![Some(credits.id), Some(hero_select.id), Some(misc_setup.id)],
        "main.ostw imports resolve to the right files"
    );

    // `../main.ostw` from interface/HeroSelect.del resolves to the root main.
    let main_import = hero_select
        .imports
        .iter()
        .find(|import| import.path == "../main.ostw")
        .expect("HeroSelect imports ../main.ostw");
    assert_eq!(
        main_import.target,
        Some(main.id),
        "../main.ostw from interface/ resolves to the root main.ostw"
    );

    // `../utils/ScreenToWorld.del` from interface/ClickArea.del.
    let click_area = by_path("interface/ClickArea.del");
    let screen_to_world = by_path("utils/ScreenToWorld.del");
    let edge = &click_area.imports[0];
    assert_eq!(edge.path, "../utils/ScreenToWorld.del");
    assert_eq!(edge.target, Some(screen_to_world.id));

    // Out-of-closure imports resolve to None (missing). The
    // `../OSTWUtils/Diagnostics.del` import in HeroSelect.del is inside a
    // block comment and is not an import.
    let out_of_closure: Vec<_> = project
        .files
        .iter()
        .filter(|file| file.source)
        .flat_map(|file| file.imports.iter())
        .filter(|import| import.target.is_none())
        .map(|import| import.path.clone())
        .collect();
    assert_eq!(
        out_of_closure,
        vec![
            "../OSTWUtils/OnScreenText.del".to_string(),
            "../OSTWUtils/Cursor.del".to_string(),
            "../OSTWUtils/StringSorting.del".to_string(),
            "customGameSettings.lobby".to_string(),
        ]
    );
}

#[test]
fn spans_map_to_the_correct_corpus_file() {
    let outcome = compile_project(&corpus_root(), "main.ostw");
    let project = outcome.project.as_ref().expect("project loads");
    for diagnostic in &outcome.diagnostics {
        let span = diagnostic.span.expect("diagnostics carry spans");
        let record = &project.files[span.file.index()];
        assert!(
            record.path == "ds.toml" || record.source,
            "span file {} must resolve to a registry file (got {})",
            span.file.index(),
            record.path
        );
    }
    // The HeroSelect missing-import diagnostics point into HeroSelect.del.
    let hero_select = project
        .files
        .iter()
        .find(|file| file.path == "interface/HeroSelect.del")
        .unwrap();
    let missing = hero_select
        .imports
        .iter()
        .filter(|import| import.target.is_none())
        .map(|import| import.span.file.index())
        .collect::<Vec<_>>();
    assert_eq!(
        missing,
        vec![hero_select.id as usize; 3],
        "OSTWUtils missing imports point at interface/HeroSelect.del"
    );
}

// -- negative fixtures -------------------------------------------------------

fn temp_project(content: Vec<(String, String)>) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-ostw-neg-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    for (path, text) in content {
        let full = dir.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, text).unwrap();
    }
    dir
}

fn compile_temp(root: &Path, main_rel: &str) -> OstwOutcome {
    let main_text = read(&root.join(main_rel));
    wright_ostw::compile(&main_text, Some(main_rel), root)
}

#[test]
fn malformed_syntax_is_a_structured_source_located_error() {
    let root = temp_project(vec![
        (
            "ds.toml".to_string(),
            "entry_point=\"main.ostw\"\n".to_string(),
        ),
        (
            "main.ostw".to_string(),
            "globalvar Number x = ;\n".to_string(),
        ),
    ]);
    let outcome = compile_temp(&root, "main.ostw");
    assert!(outcome.error.is_none(), "project still loads");
    let parse = outcome
        .diagnostics
        .iter()
        .find(|error| error.code == "ostw-parse-error")
        .expect("malformed syntax yields ostw-parse-error");
    let span = parse.span.expect("parse errors carry a span");
    assert_eq!(
        span.file,
        FileId::from_index(1),
        "the error points at main.ostw (id 1, after ds.toml id 0)"
    );
    assert_eq!(span.start.line, 1, "the error is on line 1");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn missing_import_is_structured_and_source_located() {
    let root = temp_project(vec![
        (
            "ds.toml".to_string(),
            "entry_point=\"main.ostw\"\n".to_string(),
        ),
        (
            "main.ostw".to_string(),
            "import \"missing/File.del\";\nrule: \"r\" {}\n".to_string(),
        ),
    ]);
    let outcome = compile_temp(&root, "main.ostw");
    let missing = outcome
        .diagnostics
        .iter()
        .find(|error| error.code == "ostw-missing-import")
        .expect("a missing import yields ostw-missing-import");
    let span = missing.span.expect("missing-import carries a span");
    assert_eq!(span.start.line, 1, "the import statement is on line 1");
    assert_eq!(
        span.file,
        FileId::from_index(1),
        "the diagnostic points at main.ostw"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn invalid_entry_point_is_structured() {
    let root = temp_project(vec![
        (
            "ds.toml".to_string(),
            "entry_point=\"nope.ostw\"\n".to_string(),
        ),
        ("main.ostw".to_string(), "rule: \"r\" {}\n".to_string()),
    ]);
    let outcome = compile_temp(&root, "main.ostw");
    let entry = outcome
        .diagnostics
        .iter()
        .find(|error| error.code == "ostw-entry-not-found")
        .expect("an invalid entry_point yields ostw-entry-not-found");
    assert!(entry.message.contains("nope.ostw"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unsupported_ds_toml_key_is_structured() {
    let root = temp_project(vec![
        (
            "ds.toml".to_string(),
            "entry_point=\"main.ostw\"\nout_file=\"out.ows\"\n".to_string(),
        ),
        ("main.ostw".to_string(), "rule: \"r\" {}\n".to_string()),
    ]);
    let outcome = compile_temp(&root, "main.ostw");
    let unsupported = outcome
        .diagnostics
        .iter()
        .find(|error| error.code == "ostw-ds-toml-unsupported-key")
        .expect("an unsupported ds.toml key yields ostw-ds-toml-unsupported-key");
    assert!(unsupported.message.contains("out_file"));
    let span = unsupported.span.expect("ds.toml diagnostics carry a span");
    assert_eq!(span.file, FileId::from_index(0), "points at ds.toml (id 0)");
    assert_eq!(span.start.line, 2, "the unsupported key is on line 2");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn missing_ds_toml_is_structured() {
    let root = temp_project(vec![(
        "main.ostw".to_string(),
        "rule: \"r\" {}\n".to_string(),
    )]);
    let main_text = read(&root.join("main.ostw"));
    let outcome = wright_ostw::compile(&main_text, Some("main.ostw"), &root);
    let error = outcome
        .error
        .expect("a missing ds.toml is a project-load error");
    assert_eq!(error.code, "ostw-ds-toml-missing");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn determinism_two_runs_produce_identical_outcomes() {
    let root = corpus_root();
    let first = compile_project(&root, "main.ostw");
    let second = compile_project(&root, "main.ostw");
    assert_eq!(format!("{first:?}"), format!("{second:?}"));
}
