use wright_driver::source_provider::{
    SourceCompilation, SourceLanguage, SourceProvider, SourceProviderError, SourceTarget,
};
use wright_driver::{CompilerSession, InputSpec, SessionConfig, SourceKind};

const SOURCE: &str = "variables {\n    global:\n        0: index\n        1: other\n}\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        While(Compare(Global.index, <, 3));\n            Set Global Variable(other, Distance Between(Position Of(Event Player), Vector(0, 0, 0)));\n            Set Global Variable(index, Distance Between(Position Of(Event Player), Vector(0, 0, 0)));\n            Wait(0.016, Ignore Condition);\n        End;\n    }\n}\n";

struct MappedProvider {
    compilation: Option<SourceCompilation>,
}

impl SourceProvider for MappedProvider {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::Opy
    }

    fn compile(
        &mut self,
        _target: &SourceTarget,
    ) -> Result<SourceCompilation, SourceProviderError> {
        Ok(self.compilation.take().expect("provider result"))
    }
}

#[test]
fn mapped_lint_offers_no_evaluate_once_fix() {
    let dir = std::env::temp_dir().join(format!("wright-mapped-lint-fix-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp directory");
    let entry = dir.join("main.opy");
    std::fs::write(&entry, SOURCE).expect("member source");

    let catalog = workshop_rs::catalog::Catalog::builtin_ref().expect("catalog");
    let locale = workshop_rs::catalog::Locale::new("en-US");
    let program = workshop_rs::parser::parse_with_context(SOURCE, catalog, &locale, catalog)
        .expect("fixture parses");
    let json =
        workshop_rs::MappedText::new(SOURCE, workshop_rs::SourceMap::extract(&program)).to_json();
    let mut artifact: serde_json::Value = serde_json::from_str(&json).expect("mapped JSON");
    artifact["files"] = serde_json::json!([{
        "path": url::Url::from_file_path(&entry).expect("file URI").to_string(),
    }]);
    let mapped = workshop_rs::MappedText::from_json(&artifact.to_string()).expect("mapped text");
    let provider = MappedProvider {
        compilation: Some(SourceCompilation {
            workshop_text: Some(SOURCE.to_string()),
            locale: None,
            provenance: wright_driver::SourceProvenance::Mapped(mapped.map),
            diagnostics: Vec::new(),
            source_identity: None,
        }),
    };
    let mut session = CompilerSession::with_source_provider(
        SessionConfig {
            input: InputSpec::Path(entry),
            kind: SourceKind::Opy,
            ..SessionConfig::default()
        },
        Box::new(provider),
    )
    .expect("provider session");

    let lint = session.lint();
    assert!(lint.ok, "mapped lint: {:?}", lint.diagnostics);
    let repeated = lint
        .result
        .findings
        .as_array()
        .expect("finding array")
        .iter()
        .find(|finding| finding["code"] == "repeated-value")
        .expect("repeated-value finding");
    assert!(
        repeated.get("fix").is_none(),
        "a mapped program has no authored value extents, and Evaluate Once is not OPY"
    );
    std::fs::remove_dir_all(dir).expect("remove test directory");
}
