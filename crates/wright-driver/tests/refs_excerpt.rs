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

const PAD: &str = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";

fn source() -> String {
    format!(
        "variables {{\n    global:\n        0: score\n        1: total\n}}\n\nrule (\"tick\") {{\n    event {{\n        Ongoing - Global;\n    }}\n    actions {{\n        Set Global Variable(total, 1);\n        Set Global Variable(total, Custom String(\"{PAD}{{0}}\", score, Null, Null));\n        Set Global Variable(score, 2);\n    }}\n}}\n"
    )
}

fn refs_of(session: &mut CompilerSession, name: &str) -> Vec<serde_json::Value> {
    let refs = serde_json::to_value(session.refs(name, None, None, None, None).result)
        .expect("refs result");
    refs["references"]
        .as_array()
        .expect("reference list")
        .clone()
}

fn mapped_session(
    text: &str,
    provenance: impl FnOnce(&str, &Path) -> wright_driver::SourceProvenance,
) -> CompilerSession {
    let (dir, entry) = temp_entry();
    let member = dir.join("main.opy");
    std::fs::write(&member, text).expect("member source");
    let provenance = provenance(text, &member);
    let provider = StatementProvider {
        compilation: Some(SourceCompilation {
            workshop_text: Some(text.to_string()),
            locale: None,
            provenance,
            diagnostics: Vec::new(),
            source_identity: None,
        }),
    };
    CompilerSession::with_source_provider(
        SessionConfig {
            input: InputSpec::Path(entry),
            kind: SourceKind::Opy,
            ..SessionConfig::default()
        },
        Box::new(provider),
    )
    .expect("provider session")
}

#[test]
fn mapped_statement_span_clips_to_a_window_around_the_name() {
    let text = source();
    let mut session = mapped_session(&text, statement_provenance);
    let references = refs_of(&mut session, "score");
    assert_eq!(references.len(), 3, "{references:?}");
    let long = references
        .iter()
        .find(|reference| reference["kind"] == "read")
        .expect("the read inside the long statement");
    let excerpt = long["excerpt"].as_str().expect("excerpt");
    assert!(excerpt.starts_with('…'), "{excerpt}");
    assert!(
        excerpt.contains("score"),
        "the window holds the name: {excerpt}"
    );
    assert!(!excerpt.contains('\n'), "{excerpt}");
    assert_eq!(excerpt.chars().count(), 140 + excerpt.matches('…').count());
    let short = references
        .iter()
        .find(|reference| reference["kind"] == "write")
        .expect("the write");
    assert_eq!(
        short["excerpt"], "Set Global Variable(score, 2)",
        "a span within the bound is the source text as is"
    );
}

#[test]
fn unmapped_artifact_references_carry_no_excerpt() {
    let text = source();
    let mut session = mapped_session(&text, |_, _| wright_driver::SourceProvenance::Unmapped);
    let references = refs_of(&mut session, "score");
    assert!(!references.is_empty());
    for reference in references {
        assert!(reference.get("excerpt").is_none(), "{reference}");
    }
}

#[test]
fn workshop_reference_excerpt_is_the_source_text_at_its_span() {
    let text = source();
    let dir = std::env::temp_dir().join(format!("wright-refs-excerpt-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp directory");
    let path = dir.join("tick.ws");
    std::fs::write(&path, &text).expect("source");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path),
        ..SessionConfig::default()
    })
    .expect("session");
    let references = refs_of(&mut session, "score");
    assert_eq!(references.len(), 3, "{references:?}");
    let lines: Vec<&str> = text.lines().collect();
    for reference in references {
        let span = &reference["span"];
        let at = |end: &str, part: &str| span[end][part].as_u64().expect("position") as usize;
        assert_eq!(at("start", "line"), at("end", "line"), "{reference}");
        let line = lines[at("start", "line") - 1];
        let source: String = line
            .chars()
            .skip(at("start", "col") - 1)
            .take(at("end", "col") - at("start", "col"))
            .collect();
        assert_eq!(reference["excerpt"], source.as_str(), "{reference}");
    }
}

#[test]
fn reference_without_a_source_span_has_no_excerpt() {
    let text = source();
    let mut session = mapped_session(&text, |text, authored| {
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
        artifact["spans"]
            .as_array_mut()
            .expect("span nodes")
            .retain(|node| {
                !node["node"]
                    .as_str()
                    .unwrap_or_default()
                    .starts_with("action")
            });
        let mapped =
            workshop_rs::MappedText::from_json(&artifact.to_string()).expect("mapped text");
        wright_driver::SourceProvenance::Mapped(mapped.map)
    });
    let references = refs_of(&mut session, "score");
    assert_eq!(references.len(), 3, "{references:?}");
    for reference in &references {
        if reference["kind"] == "declaration" {
            assert_eq!(reference["excerpt"], "score", "{reference}");
        } else {
            assert!(reference["span"].is_null(), "{reference}");
            assert!(reference.get("excerpt").is_none(), "{reference}");
        }
    }
}
