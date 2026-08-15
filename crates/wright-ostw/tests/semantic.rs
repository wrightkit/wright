//! Semantic regressions (#118): the pinned protect-ban entry-point reachable
//! graph resolves through the native OSTW semantic phase into frontend-neutral
//! Wright HIR that validates, boundary forms fail deterministically with
//! structured source-located diagnostics, unreachable sources never affect
//! semantic success, and the outcome is byte-stable across runs. Assertions
//! are on observable outcomes (HIR counts, diagnostics, determinism), never
//! on hardcoded parse trees.

use std::path::{Path, PathBuf};

use wright_ostw::SemanticOutcome;

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

fn compile_semantic(root: &Path, main_rel: &str) -> (wright_ostw::OstwOutcome, SemanticOutcome) {
    let main_text = read(&root.join(main_rel));
    wright_ostw::compile_with_semantics(&main_text, Some(main_rel), root)
}

#[test]
fn reachable_closure_resolves_to_valid_frontend_neutral_hir() {
    let (project, semantic) = compile_semantic(&corpus_root(), "main.ostw");
    assert!(
        project.error.is_none(),
        "project loads: {:?}",
        project.error
    );
    let hir = semantic.hir.as_ref().expect("HIR is produced");
    hir.validate().expect("lowered HIR validates");

    // The reachable closure's supported surface is present.
    assert_eq!(
        hir.globals.len(),
        54,
        "all reachable globals (incl. foreach counters)"
    );
    assert_eq!(hir.players.len(), 4, "the reachable playervars");
    assert_eq!(hir.enums.len(), 1, "the user enum Phase");
    assert_eq!(hir.functions.len(), 53, "typed/value/void-inline functions");
    assert_eq!(hir.subroutines.len(), 10, "rule-named subroutines");
    assert_eq!(hir.rules.len(), 28, "the reachable rules");

    // The explicit global id `i 127` is honored (P3a).
    let i = hir
        .globals
        .iter()
        .find(|global| global.name == "i")
        .expect("global i");
    assert_eq!(i.index, Some(127), "explicit global id 127");

    // The user enum Phase has the expected members.
    let phase = hir
        .enums
        .iter()
        .find(|enum_| enum_.name == "Phase")
        .expect("enum Phase");
    assert_eq!(
        phase
            .members
            .iter()
            .map(|member| member.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Waiting", "Protect", "Ban", "ExtraBan", "Gameplay"]
    );

    // The 3 active OSTWUtils missing imports remain explicit boundaries.
    let missing_imports = project
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "ostw-missing-import")
        .count();
    assert_eq!(missing_imports, 3);

    // Unreachable sources contribute nothing: no protectBanFull/PlayerInterface
    // defects appear in the semantic diagnostics.
    for diagnostic in &semantic.diagnostics {
        let path = diagnostic
            .span
            .and_then(|span| {
                project
                    .project
                    .as_ref()
                    .map(|p| p.files[span.file.index()].path.clone())
            })
            .unwrap_or_default();
        assert!(
            path != "protectBanFull.ostw" && path != "interface/PlayerInterface.del",
            "unreachable file {path} must not contribute: {diagnostic:?}"
        );
    }
}

#[test]
fn unsupported_reachable_boundaries_fail_deterministically() {
    let (_, semantic) = compile_semantic(&corpus_root(), "main.ostw");
    let codes: std::collections::BTreeMap<&str, usize> =
        semantic
            .diagnostics
            .iter()
            .fold(Default::default(), |mut map, d| {
                *map.entry(d.code.as_str()).or_default() += 1;
                map
            });
    // The reachable graph's boundaries: the missing OSTWUtils/Cursor surface,
    // the Math module, and class instantiation.
    assert!(
        codes.get("ostw-unsupported").copied().unwrap_or(0) >= 20,
        "Math/Cursor/new boundaries surface: {codes:?}"
    );
    assert!(
        semantic
            .diagnostics
            .iter()
            .any(|d| d.message.contains("new Cursor")),
        "class instantiation is rejected"
    );
    // Every diagnostic is source-located.
    for diagnostic in &semantic.diagnostics {
        assert!(
            diagnostic.span.is_some(),
            "every semantic diagnostic carries a span: {diagnostic:?}"
        );
    }
}

#[test]
fn semantic_outcome_is_byte_stable_across_runs() {
    let root = corpus_root();
    let first = compile_semantic(&root, "main.ostw");
    let second = compile_semantic(&root, "main.ostw");
    let first_hir = format!("{:?}", first.1.hir);
    let second_hir = format!("{:?}", second.1.hir);
    assert_eq!(first_hir, second_hir, "HIR is byte-stable");
    assert_eq!(
        format!("{:?}", first.1.diagnostics),
        format!("{:?}", second.1.diagnostics),
        "diagnostics are byte-stable"
    );
}

#[test]
fn unreachable_broken_source_does_not_affect_semantics() {
    // A project with an unreachable broken source resolves exactly like the
    // project without it (unreachable files are never semantic inputs).
    let dir = std::env::temp_dir().join(format!("wright-ostw-sem-neg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ds.toml"), "entry_point=\"main.ostw\"\n").unwrap();
    std::fs::write(
        dir.join("main.ostw"),
        "Number add(Number a): a + 1;\nrule: \"r\" { BigMessage(AllPlayers(), \"hi\"); }\n",
    )
    .unwrap();
    std::fs::write(dir.join("broken.del"), "globalvar Number x = ;\n").unwrap();

    let (with_broken, semantic) = compile_semantic(&dir, "main.ostw");
    assert!(with_broken.error.is_none());
    assert!(
        semantic.diagnostics.is_empty(),
        "unreachable broken source contributes nothing: {:?}",
        semantic.diagnostics
    );
    let hir = semantic.hir.as_ref().expect("HIR produced");
    hir.validate().expect("HIR validates");
    assert_eq!(hir.functions.len(), 1, "only the reachable function");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reachable_unsupported_form_surfaces_structured_diagnostic() {
    // A reachable `new` (class instantiation) fails at resolution with a
    // structured source-located diagnostic, not at emission.
    let dir = std::env::temp_dir().join(format!("wright-ostw-sem-new-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ds.toml"), "entry_point=\"main.ostw\"\n").unwrap();
    std::fs::write(
        dir.join("main.ostw"),
        "playervar Cursor | Number c = -1;\nrule: \"r\" Event.OngoingPlayer { c = new Cursor(1, 2, 3, 4); }\n",
    )
    .unwrap();
    let (_, semantic) = compile_semantic(&dir, "main.ostw");
    let unsupported = semantic
        .diagnostics
        .iter()
        .find(|d| d.code == "ostw-unsupported" && d.message.contains("new Cursor"))
        .expect("new Cursor is rejected during resolution");
    assert!(unsupported.span.is_some(), "diagnostic is source-located");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn every_ostw_binding_resolves_through_the_canonical_catalog() {
    // Catalog-ownership invariant (#118 AC): wright-ostw ships only OSTW
    // source-name bindings, and every binding resolves to real canonical
    // catalog data (kind/id for builtins; domain + member ids for enums).
    let catalog = wright_workshop::catalog::Catalog::builtin().expect("catalog loads");
    let en = wright_workshop::catalog::Locale::new("en-US");

    for (source, (kind, id)) in wright_ostw::signature::BUILTIN_BINDINGS {
        let entry = catalog
            .entry(*kind, id)
            .unwrap_or_else(|| panic!("builtin '{source}' -> {id:?} has no catalog entry"));
        assert!(
            entry.spelling(&en).is_some(),
            "builtin '{source}' entry '{id}' has no en-US spelling"
        );
    }

    for (source, binding) in wright_ostw::signature::ENUM_DOMAIN_BINDINGS {
        let domain = catalog.enum_domain(binding.domain).unwrap_or_else(|| {
            panic!(
                "enum domain '{source}' -> '{}' has no catalog domain",
                binding.domain
            )
        });
        for (member_source, canonical) in binding.members {
            assert!(
                domain
                    .members
                    .iter()
                    .any(|member| &member.member == canonical),
                "domain '{source}': member '{member_source}' -> '{canonical}' is not in the catalog domain '{}'",
                binding.domain
            );
        }
    }
}

#[test]
fn builtins_and_enum_domains_resolve_through_the_canonical_catalog() {
    // Representative resolutions through the shipped semantic path: an action
    // (BigMessage), a value (AllPlayers), and a builtin enum domain
    // (Color.White) resolve to HIR through the canonical Workshop catalog.
    let dir = std::env::temp_dir().join(format!("wright-ostw-sem-cat-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ds.toml"), "entry_point=\"main.ostw\"\n").unwrap();
    std::fs::write(
        dir.join("main.ostw"),
        "rule: \"r\" Event.OngoingPlayer {\n  BigMessage(AllPlayers(), \"hi\");\n  BigMessage(AllPlayers(), Color.White);\n}\n",
    )
    .unwrap();
    let (_, semantic) = compile_semantic(&dir, "main.ostw");
    assert!(
        semantic.diagnostics.is_empty(),
        "the exercised surface resolves cleanly: {:?}",
        semantic.diagnostics
    );
    let hir = semantic.hir.as_ref().expect("HIR produced");

    let calls: Vec<&str> = hir
        .exprs
        .iter()
        .filter_map(|expr| match expr {
            wright_ir::hir::Expr::Call { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert!(calls.contains(&"BigMessage"), "action resolves: {calls:?}");
    assert!(calls.contains(&"AllPlayers"), "value resolves: {calls:?}");

    let enums: Vec<(&str, &str)> = hir
        .exprs
        .iter()
        .filter_map(|expr| match expr {
            wright_ir::hir::Expr::Enum {
                value_type, value, ..
            } => Some((value_type.as_str(), value.as_str())),
            _ => None,
        })
        .collect();
    assert!(
        enums.contains(&("Color", "White")),
        "enum domain resolves: {enums:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
