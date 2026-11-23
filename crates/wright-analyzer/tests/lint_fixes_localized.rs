use workshop_rs::catalog::Locale;
use workshop_rs::parser;
use workshop_rs::source::Span;
use wright_analyzer::canonical::{Finding, LintFix, analyze};
use wright_analyzer::registry::LintConfig;

fn coded<'a>(findings: &'a [Finding], code: &str) -> Vec<&'a Finding> {
    findings
        .iter()
        .filter(|finding| finding.code == code)
        .collect()
}

fn span_text(program: &workshop_rs::Program, source: &str, span: Span) -> String {
    let range = program
        .source(span.file)
        .and_then(|document| document.byte_range(span))
        .unwrap_or_else(|| panic!("span resolves against the source: {span:?}"));
    source[range].to_string()
}

#[test]
fn duplicate_condition_fix_materializes_for_a_localized_source() {
    let localized = r#"变量 {
    全局:
        0: index
        1: other
}
规则 ("duplicates") {
    事件 {
        持续 - 全局;
    }
    动作 {
        If(比较(全局.index, ==, 0));
            设置全局变量(other, 1);
        Else If(比较(全局.index, ==, 0));
            设置全局变量(other, 2);
        Else;
            设置全局变量(other, 3);
        End;
    }
}
"#;
    let catalog = workshop_rs::catalog::Catalog::builtin_ref().unwrap();
    let program = parser::parse_with_context(localized, catalog, &Locale::new("zh-CN"), catalog)
        .unwrap_or_else(|error| panic!("zh-CN source must parse: {error}"));
    let findings = analyze(&program, &LintConfig::default());
    let [finding] = coded(&findings, "duplicate-condition")[..] else {
        panic!("one duplicate-condition finding: {findings:?}")
    };
    let Some(LintFix::RemoveDeadBranch { span }) = &finding.fix else {
        panic!("the finding carries a dead-branch fix: {finding:?}")
    };
    let removed = span_text(&program, localized, *span);
    assert!(
        removed.contains("Else If(比较(全局.index, ==, 0))")
            && removed.contains("设置全局变量(other, 2)"),
        "the removal covers the localized dead branch: {removed:?}"
    );
    assert!(
        !removed.contains("Else;") && !removed.contains("设置全局变量(other, 3)"),
        "the fix stops at the Else marker: {removed:?}"
    );
}

#[test]
fn duplicate_condition_fix_spans_match_for_es_mx_control_flow_keywords() {
    let english = |tail: &str| {
        format!(
            "variables {{\n    global:\n        0: index\n        1: other\n}}\nrule (\"chain\") {{\n    event {{\n        Ongoing - Global;\n    }}\n    actions {{\n        If(Compare(Global.index, ==, 0));\n            Set Global Variable(other, 1);\n        Else If(Compare(Global.index, ==, 0));\n            Set Global Variable(other, 2);\n{tail}    }}\n}}\n"
        )
    };
    let localized = |tail: &str| {
        format!(
            "variables {{\n    global:\n        0: index\n        1: other\n}}\nrule (\"chain\") {{\n    event {{\n        Ongoing - Global;\n    }}\n    actions {{\n        Si(Comparar(Global.index, ==, 0));\n            Establecer variable global(other, 1);\n        Si no si(Comparar(Global.index, ==, 0));\n            Establecer variable global(other, 2);\n{tail}    }}\n}}\n"
        )
    };
    let fix_span = |source: &str, locale: &str| {
        let catalog = workshop_rs::catalog::Catalog::builtin_ref().unwrap();
        let program = parser::parse_with_context(source, catalog, &Locale::new(locale), catalog)
            .unwrap_or_else(|error| panic!("{locale} source must parse: {error}"));
        let findings = analyze(&program, &LintConfig::default());
        let [finding] = coded(&findings, "duplicate-condition")[..] else {
            panic!("one duplicate-condition finding for {locale}: {findings:?}")
        };
        let Some(LintFix::RemoveDeadBranch { span }) = &finding.fix else {
            panic!("the {locale} finding carries a dead-branch fix: {finding:?}")
        };
        (span.start.line, span.start.col, span.end.line, span.end.col)
    };
    for (english_tail, localized_tail) in [
        (
            "        Else;\n            Set Global Variable(other, 3);\n        End;\n",
            "        Si no;\n            Establecer variable global(other, 3);\n        Fin;\n",
        ),
        (
            "        Else If(Compare(Global.index, ==, 2));\n            Set Global Variable(other, 3);\n        End;\n",
            "        Si no si(Comparar(Global.index, ==, 2));\n            Establecer variable global(other, 3);\n        Fin;\n",
        ),
        ("        End;\n", "        Fin;\n"),
    ] {
        let expected = fix_span(&english(english_tail), "en-US");
        assert_eq!(
            fix_span(&localized(localized_tail), "es-MX"),
            expected,
            "the localized chain fixes the same lines as the English one"
        );
    }
}
