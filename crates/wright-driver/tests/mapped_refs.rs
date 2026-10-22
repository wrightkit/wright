//! #892: a provider's map is statement-grain — workshop-rs exposes no
//! setters for identifier-level slots, so a real provider can never carry
//! them. References still resolve to the enclosing statement's span, the
//! same `at=` `inspect cost` reports.

use std::path::{Path, PathBuf};

use wright_driver::source_provider::{
    SourceCompilation, SourceLanguage, SourceProvider, SourceProviderError, SourceTarget,
};
use wright_driver::{CompilerSession, InputSpec, SessionConfig, SourceKind};

fn temp_entry() -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("wright-mapped-refs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp directory");
    let entry = dir.join("main.opy");
    std::fs::write(&entry, "this is intentionally not native OPY").expect("entry source");
    (dir, entry)
}

struct StatementProvider {
    compilation: Option<SourceCompilation>,
}

impl SourceProvider for StatementProvider {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::Opy
    }

    fn check(&mut self, target: &SourceTarget) -> Result<SourceCompilation, SourceProviderError> {
        self.compile(target)
    }

    fn compile(
        &mut self,
        _target: &SourceTarget,
    ) -> Result<SourceCompilation, SourceProviderError> {
        Ok(self.compilation.take().expect("provider result"))
    }
}

fn statement_provenance(text: &str, authored: &Path) -> wright_driver::SourceProvenance {
    let catalog = workshop_rs::catalog::Catalog::builtin_ref().expect("catalog");
    let locale = workshop_rs::catalog::Locale::new("en-US");
    let program = workshop_rs::parser::parse_with_context(text, catalog, &locale, catalog)
        .expect("fixture parses");
    let json =
        workshop_rs::MappedText::new(text, workshop_rs::SourceMap::extract(&program)).to_json();
    let mut artifact: serde_json::Value = serde_json::from_str(&json).expect("mapped JSON");
    artifact["files"] = serde_json::json!([{
        "path": url::Url::from_file_path(authored).expect("file URI").to_string(),
    }]);
    for node in artifact["spans"].as_array_mut().expect("span nodes") {
        let node = node.as_object_mut().expect("node");
        node.remove("identifier_span");
        node.remove("event_name_span");
        node.remove("children");
        if node["node"] == "rule" {
            node.remove("name_span");
        }
    }
    let mapped = workshop_rs::MappedText::from_json(&artifact.to_string()).expect("mapped text");
    wright_driver::SourceProvenance::Mapped(mapped.map)
}

#[test]
fn mapped_refs_fall_back_to_the_statement_span() {
    const SOURCE: &str = "variables {\n    global:\n        0: score\n}\n\nsubroutines {\n    0: helper\n}\n\nrule (\"worker\") {\n    event {\n        Subroutine;\n        helper;\n    }\n    actions {\n        Set Global Variable(score, 1);\n    }\n}\n\nrule (\"tick\") {\n    event {\n        Ongoing - Global;\n    }\n    conditions {\n        score == 0;\n    }\n    actions {\n        Call Subroutine(helper);\n    }\n}\n\nrule (\"tick\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(score, 2);\n    }\n}\n";
    let (dir, entry) = temp_entry();
    let member = dir.join("main.opy");
    std::fs::write(&member, SOURCE).expect("member source");
    let provenance = statement_provenance(SOURCE, &member);
    let provider = StatementProvider {
        compilation: Some(SourceCompilation {
            workshop_text: Some(SOURCE.to_string()),
            locale: None,
            provenance,
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

    // Read in a condition plus writes in two actions — every reference
    // resolves the enclosing statement's authored span.
    let refs = serde_json::to_value(session.refs("score", None, None, None, None).result)
        .expect("refs result");
    let references = refs["references"].as_array().expect("reference list");
    assert_eq!(references.len(), 4, "{references:?}");
    for reference in references {
        assert_eq!(
            reference["span"]["path"].as_str().unwrap_or_default(),
            "main.opy",
            "every reference resolves the authored span: {reference}"
        );
        assert!(
            reference["span"]["start"]["line"].as_u64().unwrap_or(0) > 0,
            "{reference}"
        );
    }
    // ADR-0035: the two `tick` rules keep distinct canonical addresses.
    let addresses: Vec<_> = references
        .iter()
        .filter_map(|reference| reference["owningRule"]["address"].as_str())
        .collect();
    assert!(addresses.contains(&"tick#1"), "{addresses:?}");
    assert!(addresses.contains(&"tick#2"), "{addresses:?}");

    // Call references and the subroutine definition resolve spans too.
    let refs = serde_json::to_value(session.refs("helper", None, None, None, None).result)
        .expect("subroutine refs");
    for reference in refs["references"].as_array().expect("reference list") {
        assert!(
            reference["span"]["path"].as_str().unwrap_or_default() == "main.opy"
                && reference["span"]["start"]["line"].as_u64().unwrap_or(0) > 0,
            "call and definition references resolve a span: {reference}"
        );
    }
    std::fs::remove_dir_all(dir).expect("remove test directory");
}
