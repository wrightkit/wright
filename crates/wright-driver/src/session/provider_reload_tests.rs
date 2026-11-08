use super::*;
use crate::source_provider::{SourceCompilation, SourceLanguage, SourceTarget};

struct FixtureProvider(String);

impl SourceProvider for FixtureProvider {
    fn language(&self) -> crate::source_provider::SourceLanguage {
        SourceLanguage::Opy
    }

    fn compile(
        &mut self,
        _target: &SourceTarget,
    ) -> Result<SourceCompilation, SourceProviderError> {
        Ok(SourceCompilation::success(self.0.clone()))
    }
}

#[test]
fn provider_reload_succeeds_after_compile_and_drop() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text =
        std::fs::read_to_string(root.join("tests/fixtures/workshop/synthetic/basic-rule.ws"))
            .expect("Workshop fixture");
    let dir = root.join("target").join("provider-reload-test");
    std::fs::create_dir_all(&dir).expect("test directory");
    let entry = dir.join("main.opy");
    std::fs::write(&entry, "// provider-owned source").expect("entry");
    let config = SessionConfig {
        input: InputSpec::Path(entry),
        kind: SourceKind::Opy,
        ..SessionConfig::default()
    };
    let mut session =
        CompilerSession::with_source_provider(config, Box::new(FixtureProvider(text)))
            .expect("provider session");
    assert!(session.compile().ok);
    session.drop_loaded();
    assert!(session.cached_loaded().is_none());
    session.reload().expect("reload re-compiles after drop");
}
