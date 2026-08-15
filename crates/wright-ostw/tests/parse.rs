//! Corpus-driven frontend regressions (#117): the real committed protect-ban
//! project must load with compilation membership equal to the `ds.toml`
//! entry-point import closure, quoted imports resolve with correct
//! multi-file provenance, unreachable sources never contribute project
//! diagnostics, and deterministic negative fixtures cover malformed syntax,
//! missing imports, invalid entry points, unsupported ds.toml configuration,
//! reachability flips, and cycle/duplicate determinism. Assertions are on
//! observable outcomes, never on hardcoded parse trees.

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

/// The 16 committed protect-ban source files in the workspace inventory.
const PROTECT_BAN_INVENTORY: &[&str] = &[
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

/// The 7 entry-point import-reachable files of protect-ban (`main.ostw`).
const PROTECT_BAN_CLOSURE: &[&str] = &[
    "main.ostw",
    "Credits.ostw",
    "interface/HeroSelect.del",
    "interface/miscSetup.del",
    "interface/HeroSelectConfig.del",
    "interface/HeroSelectFunctions.del",
    "interface/MapData.del",
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
fn protect_ban_compilation_membership_is_the_entry_point_closure() {
    // Compilation membership = the entry-point import closure: exactly the 7
    // reachable files parse; unreachable-file defects contribute nothing.
    let outcome = compile_project(&corpus_root(), "main.ostw");
    assert!(
        outcome.error.is_none(),
        "project must load: {:?}",
        outcome.error
    );
    let project = outcome.project.as_ref().expect("project loads");
    assert_eq!(project.entry, "main.ostw", "ds.toml entry_point");

    let files = sources(&outcome);
    assert_eq!(
        files.len(),
        PROTECT_BAN_CLOSURE.len(),
        "only the import-reachable closure is compiled"
    );
    for source in PROTECT_BAN_CLOSURE {
        let record = files
            .iter()
            .find(|file| file.path == *source)
            .unwrap_or_else(|| panic!("missing closure source {source} in the registry"));
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

    // Exactly the 3 reachable OSTWUtils missing imports appear. Unreachable
    // defects (e.g. protectBanFull.ostw's `customGameSettings.lobby` import)
    // contribute nothing.
    let missing: Vec<_> = outcome
        .diagnostics
        .iter()
        .filter(|error| error.code == "ostw-missing-import")
        .collect();
    assert_eq!(
        missing.len(),
        3,
        "only reachable missing imports appear: {:?}",
        outcome.diagnostics
    );
    for diagnostic in &missing {
        assert!(
            diagnostic.span.is_some(),
            "missing-import diagnostics carry a source location"
        );
    }

    // The workspace inventory is distinct and retains all 16 sources.
    assert_eq!(
        project.inventory, PROTECT_BAN_INVENTORY,
        "the inventory is the full workspace source list"
    );
}

#[test]
fn protect_ban_inventory_parses_for_robustness_only() {
    // All-protect-ban-files parser robustness: every inventory source lexes
    // and parses cleanly through the shipped frontend functions. This is a
    // parser-robustness check over the inventory, not a compilation-membership
    // or project-success assertion (unreachable files are not compilation
    // members).
    for source in PROTECT_BAN_INVENTORY {
        let path = corpus_root().join(source);
        let text = read(&path);
        let tokens = wright_ostw::lexer::lex(wright_ostw::lexer::LexInput {
            file_id: FileId::from_index(0),
            text: &text,
        })
        .unwrap_or_else(|error| panic!("{source} must lex: {error}"));
        wright_ostw::parser::parse(tokens)
            .unwrap_or_else(|error| panic!("{source} must parse: {error}"));
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

    // The closure contains each file exactly once despite the corpus's
    // import cycles (main <-> HeroSelect/miscSetup, HeroSelect <->
    // HeroSelectConfig <-> HeroSelectFunctions).
    assert_eq!(sources(&outcome).len(), 7, "each file appears once");

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

fn project_of(outcome: &OstwOutcome) -> &wright_ostw::Project {
    outcome.project.as_ref().expect("project loads")
}

fn source_paths(outcome: &OstwOutcome) -> Vec<String> {
    project_of(outcome)
        .files
        .iter()
        .filter(|file| file.source)
        .map(|file| file.path.clone())
        .collect()
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

// -- compilation-graph regressions -------------------------------------------

#[test]
fn unreachable_broken_source_does_not_fail_the_project() {
    // A source with broken syntax that is not reachable from the entry must
    // not fail the project or produce any diagnostic.
    let root = temp_project(vec![
        (
            "ds.toml".to_string(),
            "entry_point=\"main.ostw\"\n".to_string(),
        ),
        ("main.ostw".to_string(), "rule: \"r\" {}\n".to_string()),
        (
            "broken.ostw".to_string(),
            "globalvar Number x = ;\n".to_string(),
        ),
    ]);
    let outcome = compile_temp(&root, "main.ostw");
    assert!(outcome.error.is_none());
    assert!(
        outcome.diagnostics.is_empty(),
        "unreachable broken syntax contributes nothing: {:?}",
        outcome.diagnostics
    );
    assert_eq!(
        source_paths(&outcome),
        vec!["main.ostw"],
        "only the entry is a compilation member"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn unreachable_missing_import_does_not_fail_the_project() {
    // A source with a missing import that is not reachable from the entry
    // must not produce a missing-import diagnostic.
    let root = temp_project(vec![
        (
            "ds.toml".to_string(),
            "entry_point=\"main.ostw\"\n".to_string(),
        ),
        ("main.ostw".to_string(), "rule: \"r\" {}\n".to_string()),
        (
            "orphan.del".to_string(),
            "import \"gone/File.del\";\nrule: \"o\" {}\n".to_string(),
        ),
    ]);
    let outcome = compile_temp(&root, "main.ostw");
    assert!(outcome.error.is_none());
    assert!(
        outcome.diagnostics.is_empty(),
        "unreachable missing imports contribute nothing: {:?}",
        outcome.diagnostics
    );
    assert_eq!(source_paths(&outcome), vec!["main.ostw"]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn making_a_source_reachable_surfaces_its_diagnostic() {
    // The reachability flip: an unreachable source's defect is hidden; once
    // the entry closure imports it, the structured source-located diagnostic
    // appears; removing the import hides it again.
    let mk = |import: bool| {
        let mut content = vec![
            (
                "ds.toml".to_string(),
                "entry_point=\"main.ostw\"\n".to_string(),
            ),
            (
                "main.ostw".to_string(),
                if import {
                    "import \"broken.del\";\nrule: \"r\" {}\n".to_string()
                } else {
                    "rule: \"r\" {}\n".to_string()
                },
            ),
            (
                "broken.del".to_string(),
                "import \"gone/File.del\";\nrule: \"b\" {}\n".to_string(),
            ),
        ];
        // `gone/File.del` intentionally does not exist: broken.del's defect
        // is the missing import, surfaced only once broken.del is reachable.
        if !import {
            content.push(("unused.del".to_string(), "Number x: 1;\n".to_string()));
        }
        temp_project(content)
    };

    let hidden = compile_temp(&mk(false), "main.ostw");
    assert!(
        hidden.diagnostics.is_empty(),
        "unreachable: no diagnostic: {:?}",
        hidden.diagnostics
    );
    assert_eq!(source_paths(&hidden), vec!["main.ostw"]);

    let visible = compile_temp(&mk(true), "main.ostw");
    let missing = visible
        .diagnostics
        .iter()
        .find(|error| error.code == "ostw-missing-import")
        .expect("reachable broken import surfaces ostw-missing-import");
    let span = missing.span.expect("diagnostic is source-located");
    assert_eq!(span.file, FileId::from_index(2), "points at broken.del");
    assert_eq!(
        source_paths(&visible),
        vec!["main.ostw", "broken.del"],
        "broken.del is now a compilation member"
    );

    let _ = std::fs::remove_dir_all(mk(false));
    let _ = std::fs::remove_dir_all(mk(true));
}

#[test]
fn cycles_and_duplicate_imports_are_deterministic() {
    // A cycle (a <-> b) plus duplicate imports must include each file once,
    // produce no duplicate diagnostics, and be byte-stable across loads.
    let root = temp_project(vec![
        (
            "ds.toml".to_string(),
            "entry_point=\"main.ostw\"\n".to_string(),
        ),
        (
            "main.ostw".to_string(),
            "import \"a.del\";\nimport \"a.del\";\nimport \"b.del\";\nrule: \"m\" {}\n".to_string(),
        ),
        (
            "a.del".to_string(),
            "import \"b.del\";\nrule: \"a\" {}\n".to_string(),
        ),
        (
            "b.del".to_string(),
            "import \"a.del\";\nrule: \"b\" {}\n".to_string(),
        ),
    ]);
    let first = compile_temp(&root, "main.ostw");
    let second = compile_temp(&root, "main.ostw");
    assert_eq!(format!("{first:?}"), format!("{second:?}"), "byte-stable");

    assert!(first.error.is_none());
    assert!(
        first.diagnostics.is_empty(),
        "no duplicate diagnostics from the cycle/duplicates: {:?}",
        first.diagnostics
    );
    assert_eq!(
        source_paths(&first),
        vec!["main.ostw", "a.del", "b.del"],
        "each file appears exactly once"
    );
    // The duplicate `import "a.del"` produces two resolved edges, both to the
    // same file id — never a diagnostic.
    let main = project_of(&first)
        .files
        .iter()
        .find(|file| file.path == "main.ostw")
        .unwrap();
    assert_eq!(
        main.imports
            .iter()
            .filter(|import| import.path == "a.del")
            .count(),
        2
    );
    let a = project_of(&first)
        .files
        .iter()
        .find(|file| file.path == "a.del")
        .unwrap();
    assert_eq!(
        a.imports[0].target,
        Some(
            project_of(&first)
                .files
                .iter()
                .find(|file| file.path == "b.del")
                .unwrap()
                .id
        ),
        "cycle edge a -> b resolves"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn determinism_two_runs_produce_identical_outcomes() {
    let root = corpus_root();
    let first = compile_project(&root, "main.ostw");
    let second = compile_project(&root, "main.ostw");
    assert_eq!(format!("{first:?}"), format!("{second:?}"));
}

#[test]
fn overlays_validate_proposed_edits_without_rewriting_files() {
    // M14 #128: a proposed multi-file edit validates against overlay text
    // (main file and imports) while the on-disk project stays untouched.
    use std::collections::BTreeMap;
    let root = temp_project(vec![
        (
            "ds.toml".to_string(),
            "entry_point=\"main.ostw\"\n".to_string(),
        ),
        (
            "main.ostw".to_string(),
            "import \"lib.del\";\nrule: \"main\" {}\n".to_string(),
        ),
        ("lib.del".to_string(), "rule: \"lib\" {}\n".to_string()),
    ]);
    let original_main = read(&root.join("main.ostw"));

    // A clean overlay of both files compiles with no diagnostics, and the
    // overlay text — not the disk content — is what parsed.
    let overlay: BTreeMap<String, String> = BTreeMap::from([
        (
            "main.ostw".to_string(),
            "import \"lib.del\";\nrule: \"edited main\" {}\n".to_string(),
        ),
        (
            "lib.del".to_string(),
            "rule: \"edited lib\" {}\n".to_string(),
        ),
    ]);
    let outcome =
        wright_ostw::compile_with_overlay(&original_main, Some("main.ostw"), &root, &overlay);
    assert!(
        outcome.error.is_none(),
        "project must load: {:?}",
        outcome.error
    );
    assert!(
        outcome.diagnostics.is_empty(),
        "clean overlay edits compile: {:?}",
        outcome.diagnostics
    );
    let main = project_of(&outcome)
        .files
        .iter()
        .find(|file| file.path == "main.ostw")
        .expect("main.ostw in the registry");
    let edited_name = main
        .cst
        .as_ref()
        .expect("main.ostw parsed")
        .items
        .iter()
        .filter_map(|item| match item {
            wright_ostw::cst::Item::Rule(rule) => rule.name.clone(),
            _ => None,
        })
        .next();
    assert_eq!(
        edited_name.as_deref(),
        Some("edited main"),
        "the overlay main text parsed"
    );

    // A broken overlay edit refuses with a source-located error naming the
    // overlaid file — the proposed edit was validated, never the disk file.
    let broken: BTreeMap<String, String> =
        BTreeMap::from([("lib.del".to_string(), "rule: \"broken {}\n".to_string())]);
    let outcome =
        wright_ostw::compile_with_overlay(&original_main, Some("main.ostw"), &root, &broken);
    assert!(outcome.error.is_none(), "project still loads");
    assert!(
        !outcome.diagnostics.is_empty(),
        "broken overlay edit yields diagnostics, got: {:?}",
        outcome.diagnostics
    );
    let parse = outcome
        .diagnostics
        .iter()
        .find(|error| error.code == "ostw-parse-error" || error.code == "ostw-lex-error")
        .expect("broken overlay edit yields a parse/lex error");
    let span = parse.span.expect("parse errors carry a span");
    let lib = project_of(&outcome)
        .files
        .iter()
        .find(|file| file.path == "lib.del")
        .expect("lib.del in the registry");
    assert_eq!(
        span.file,
        FileId::from_index(lib.id as usize),
        "the error points at the overlaid lib.del, not the main file"
    );

    // The overlay main text takes precedence over the passed-in main text.
    let overlay_main: BTreeMap<String, String> = BTreeMap::from([(
        "main.ostw".to_string(),
        "import \"lib.del\";\nrule: \"overlaid\" {}\n".to_string(),
    )]);
    let outcome = wright_ostw::compile_with_overlay(
        "rule: \"broken main {}\n",
        Some("main.ostw"),
        &root,
        &overlay_main,
    );
    assert!(
        outcome.diagnostics.is_empty(),
        "overlay main text wins over the passed-in main text: {:?}",
        outcome.diagnostics
    );

    // The on-disk files were never rewritten.
    assert_eq!(
        read(&root.join("main.ostw")),
        original_main,
        "disk main.ostw unchanged"
    );
    let _ = std::fs::remove_dir_all(&root);
}
