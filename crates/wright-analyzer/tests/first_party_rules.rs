//! The embedded first-party package (`wrightkit/lint-rules`, #1081): its
//! rules carry the removed native rules' span sets, and a pinned revision
//! replaces it.

use std::collections::BTreeSet;
use std::path::Path;

use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::{Program, parser};
use wright_analyzer::registry::{LintConfig, LintRegistry, RuleRegistryError};

fn parse_workshop(source: &str) -> Program {
    let catalog = Catalog::builtin_ref().unwrap();
    parser::parse_with_context(source, catalog, &Locale::new("en-US"), catalog)
        .unwrap_or_else(|error| panic!("Workshop source must parse: {error}"))
}

#[test]
fn a_pinned_first_party_package_replaces_the_embedded_one() {
    let mut registry = LintRegistry::default();
    let embedded: Vec<_> = registry
        .descriptors(&LintConfig::default())
        .into_iter()
        .filter(|d| d.kind == "declarative")
        .map(|d| d.id)
        .collect();
    assert_eq!(
        embedded,
        [
            "expensive-loop-check",
            "min-wait-loop",
            "nested-binder-lambda"
        ]
    );
    let pinned = "id: min-wait-loop\nmetadata:\n  summary: pinned\n  rationale: r\n  documentation: d\n  known-limits: k\n  tags: []\nmatcher: {}\n";
    assert!(
        matches!(
            registry.load_first_party_yaml_str(pinned),
            Err(RuleRegistryError::DuplicateId(id)) if id == "min-wait-loop"
        ),
        "first-party rules collide with the embedded package unless it is removed"
    );
    registry.remove_embedded_first_party();
    registry.load_first_party_yaml_str(pinned).unwrap();
    let descriptors = registry.descriptors(&LintConfig::default());
    let declarative: Vec<_> = descriptors
        .iter()
        .filter(|d| d.kind == "declarative")
        .map(|d| (d.id.as_str(), d.summary.as_str()))
        .collect();
    assert_eq!(
        declarative,
        [("min-wait-loop", "pinned")],
        "the pinned revision's rules replace the whole embedded package"
    );
}

/// One finding's span and addressed action as `(start line, start col, end
/// line, end col, action)`. `action` is part of the machine-readable payload
/// (`wright inspect cost`), so parity covers the addressed node as well as
/// the span.
type SpanKey = (u32, u32, u32, u32, Option<usize>);

fn span_set(findings: &[wright_analyzer::canonical::Finding], code: &str) -> BTreeSet<SpanKey> {
    findings
        .iter()
        .filter(|finding| finding.code == code)
        .map(|finding| {
            let span = finding.span.expect("every finding carries a span");
            (
                span.start.line,
                span.start.col,
                span.end.line,
                span.end.col,
                finding.action,
            )
        })
        .collect()
}

/// The embedded first-party rules report the span sets the removed native
/// rules reported (#970 parity, carried by #1081). The expected sets were
/// recorded from the native implementation at wright 0.42.0 before its
/// removal.
fn assert_native_span_sets(program: &Program, expected: [(&str, &[SpanKey]); 3]) {
    let findings = LintRegistry::default().run(program, &LintConfig::default());
    for (code, spans) in expected {
        assert_eq!(
            span_set(&findings, code),
            spans.iter().copied().collect::<BTreeSet<_>>(),
            "{code} span parity with the removed native rule"
        );
    }
}

#[test]
fn embedded_rules_report_the_native_span_set_on_the_bastion_reproducer() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/workshop/real-world/reproducer-bastion-prophet.ws");
    let program = parse_workshop(&std::fs::read_to_string(&path).unwrap());
    // 16 `nested-binder-lambda` findings, as workshop-rs
    // `docs/import-limits.md` records for the reproducer (the #1081 body's
    // "13" predates the final fixture).
    let binder: Vec<SpanKey> = (0..8)
        .flat_map(|action| {
            let line = 22 + action as u32;
            let (first, second) = if action == 0 {
                ((405, 419), (513, 527))
            } else {
                ((399, 413), (504, 518))
            };
            [
                (line, first.0, line, first.1, Some(action)),
                (line, second.0, line, second.1, Some(action)),
            ]
        })
        .collect();
    assert_eq!(binder.len(), 16);
    assert_native_span_sets(
        &program,
        [
            ("nested-binder-lambda", &binder),
            ("expensive-loop-check", &[]),
            ("min-wait-loop", &[]),
        ],
    );
}

#[test]
fn embedded_rules_report_the_native_span_set_on_lint_fixtures() {
    let source = r#"
variables {
    global:
        0: index
        1: pool
        2: flag
}
rule ("parity") {
    event {
        Ongoing - Global;
    }
    conditions {
        Count Of(Filtered Array(All Players(Team 1), Is True For Any(Array(Is Alive(Current Array Element)), Current Array Element))) > 0;
    }
    actions {
        Set Global Variable(pool, Filtered Array(All Players(Team 1), Is True For All(Array(Is Alive(Current Array Element)), Current Array Element)));
        Set Global Variable(flag, Filtered Array(All Players(Team 1), Compare(Filtered Array(All Players(Team 2), Is Alive(Current Array Element)), !=, Empty Array)));
        While(Compare(Global.index, <, 3));
            Set Global Variable(index, Distance Between(Position Of(Event Player), Vector(0, 0, 0)));
            If(Compare(Global.index, ==, 1));
                Set Global Variable(flag, Ray Cast Hit Position(Eye Position(Event Player), Direction Of(Angles(Global.index)), All Players(All Teams), Event Player, False));
            End;
            Wait(0.016, Ignore Condition);
        End;
        For Global Variable(index, 0, 3, 1);
            Wait(0.016, Ignore Condition);
        End;
        For Player Variable(Event Player, index, 0, 3, 1);
            Set Global Variable(index, Add(Is In Line of Sight(Eye Position(Event Player), Eye Position(Event Player), All Players(All Teams)), 1));
            Wait(0.5, Ignore Condition);
        End;
    }
}
"#;
    assert_native_span_sets(
        &parse_workshop(source),
        [
            (
                "nested-binder-lambda",
                &[
                    (13, 54, 13, 69, None),
                    (16, 71, 16, 86, Some(0)),
                    (17, 79, 17, 93, Some(1)),
                ],
            ),
            (
                "expensive-loop-check",
                &[(19, 40, 19, 56, Some(2)), (29, 44, 29, 63, Some(12))],
            ),
            (
                "min-wait-loop",
                &[(18, 9, 24, 12, Some(2)), (25, 9, 27, 12, Some(9))],
            ),
        ],
    );
}
