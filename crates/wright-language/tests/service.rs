use std::path::PathBuf;

use wright_language::LanguageService;
use wright_language::document::Document;

fn service() -> LanguageService {
    LanguageService::new(PathBuf::from("."))
}

#[test]
fn source_documents_report_provider_boundary_without_static_fallback() {
    let mut service = service();
    for ext in ["opy", "ostw", "del"] {
        let uri = format!("file:///workspace/main.{ext}");
        service.store.open(Document::new(
            uri.clone(),
            "globalvar score = 0\n".to_string(),
            PathBuf::from("."),
        ));
        let diagnostics = service.diagnostics(&uri);
        assert_eq!(
            diagnostics[0].code, "source-provider-unavailable",
            "{ext} documents surface an explicit provider refusal"
        );
    }
}

#[test]
fn workshop_documents_publish_no_provider_diagnostics() {
    let mut service = service();
    let uri = "file:///workspace/main.ws";
    service.store.open(Document::new(
        uri.to_string(),
        "rule(\"demo\") {\n    event {\n        Ongoing - Global;\n    }\n}\n".to_string(),
        PathBuf::from("."),
    ));
    assert!(
        service.diagnostics(uri).is_empty(),
        "a raw Workshop document does not require a source provider"
    );
}
