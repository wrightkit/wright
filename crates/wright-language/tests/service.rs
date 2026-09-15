use std::path::PathBuf;

use wright_language::LanguageService;
use wright_language::document::{Document, Position};

fn service() -> LanguageService {
    LanguageService::new(PathBuf::from("."))
}

#[test]
fn opy_language_service_reports_provider_boundary_without_static_fallback() {
    let mut service = service();
    let document = Document::new(
        "file:///workspace/main.opy".to_string(),
        "globalvar score = 0\n".to_string(),
        PathBuf::from("."),
    );
    service.store.open(document);
    let diagnostics = service.diagnostics("file:///workspace/main.opy");
    assert_eq!(diagnostics[0].code, "source-provider-unavailable");
    assert!(
        service
            .completion(
                "file:///workspace/main.opy",
                Position {
                    line: 0,
                    character: 0
                }
            )
            .is_empty()
    );
    assert!(
        service
            .semantic_tokens("file:///workspace/main.opy")
            .is_empty()
    );
}

#[test]
fn opy_rename_refuses_without_provider_editor_capability() {
    let mut service = service();
    service.store.open(Document::new(
        "file:///workspace/main.opy".to_string(),
        "globalvar score = 0\n".to_string(),
        PathBuf::from("."),
    ));
    let result = service.rename(
        "file:///workspace/main.opy",
        Position {
            line: 0,
            character: 10,
        },
        "total",
    );
    assert!(!result.ok);
    assert!(result.diagnostics[0].starts_with("source-provider-unavailable:"));
}
