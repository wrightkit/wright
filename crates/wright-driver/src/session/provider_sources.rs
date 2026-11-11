use std::io;
use std::path::Path;

use crate::trace::{Span, Trace};

pub(super) fn parse_with_sources<T>(
    files: &[String],
    root: &Path,
    trace: &Trace,
    parse: impl FnOnce() -> T,
) -> (T, Vec<io::Result<String>>) {
    if files.is_empty() {
        return (parse(), Vec::new());
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let span = trace.parallel_span("source.read");
        std::thread::scope(|scope| {
            let reads = scope.spawn(|| read_sources(files, root, span));
            let parsed = parse();
            (parsed, reads.join().expect("source reader panicked"))
        })
    }
    #[cfg(target_arch = "wasm32")]
    {
        let parsed = parse();
        (parsed, read_sources(files, root, trace.span("source.read")))
    }
}

fn read_sources(files: &[String], root: &Path, span: Span) -> Vec<io::Result<String>> {
    let read = |members: &[String]| {
        members
            .iter()
            .map(|member| std::fs::read_to_string(root.join(member)))
            .collect::<Vec<_>>()
    };
    #[cfg(not(target_arch = "wasm32"))]
    let sources = {
        std::thread::scope(|scope| {
            let readers: Vec<_> = files
                .chunks(files.len().div_ceil(8))
                .map(|members| scope.spawn(move || read(members)))
                .collect();
            readers
                .into_iter()
                .flat_map(|reader| reader.join().expect("source reader panicked"))
                .collect::<Vec<_>>()
        })
    };
    #[cfg(target_arch = "wasm32")]
    let sources = read(files);
    span.fact("files", files.len());
    span.fact(
        "bytes",
        sources
            .iter()
            .filter_map(|s| s.as_ref().ok())
            .map(String::len)
            .sum::<usize>(),
    );
    sources
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreadable_member_diagnostic_keeps_source_map_order() {
        use crate::source_provider::{
            SourceCompilation, SourceLanguage, SourceProvenance, SourceProvider,
            SourceProviderError, SourceTarget,
        };
        use crate::{CompilerSession, InputSpec, SessionConfig, SourceKind};

        struct MappedProvider(SourceCompilation);
        impl SourceProvider for MappedProvider {
            fn language(&self) -> SourceLanguage {
                SourceLanguage::Opy
            }
            fn compile(
                &mut self,
                _: &SourceTarget,
            ) -> Result<SourceCompilation, SourceProviderError> {
                Ok(self.0.clone())
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path().join("main.opy");
        std::fs::write(&entry, "provider-owned source").unwrap();
        let source = "rule (\"tick\") { event { Ongoing - Global; } actions { Wait(1, Ignore Condition); } }";
        let catalog = workshop_rs::catalog::Catalog::builtin_ref().unwrap();
        let locale = workshop_rs::catalog::Locale::new("en-US");
        let program =
            workshop_rs::parser::parse_with_context(source, catalog, &locale, catalog).unwrap();
        let mapped =
            workshop_rs::MappedText::new(source, workshop_rs::SourceMap::extract(&program));
        let mut artifact: serde_json::Value = serde_json::from_str(&mapped.to_json()).unwrap();
        let members = [
            entry.clone(),
            dir.path().join("z-missing.opy"),
            dir.path().join("a-invalid.opy"),
        ];
        std::fs::write(&members[2], [0xff]).unwrap();
        artifact["files"] = members
            .iter()
            .map(|path| {
                serde_json::json!({
                    "path": url::Url::from_file_path(path).unwrap().to_string(),
                })
            })
            .collect();
        let mapped = workshop_rs::MappedText::from_json(&artifact.to_string()).unwrap();
        let expected_members: Vec<_> = members[1..]
            .iter()
            .map(|path| {
                format!(
                    "{} ({})",
                    path.display(),
                    std::fs::read_to_string(path).unwrap_err()
                )
            })
            .collect();
        let expected = format!(
            "2 provider source-map member(s) could not be read, so their source extents are unavailable: {}",
            expected_members.join(", "),
        );
        let mut session = CompilerSession::with_source_provider(
            SessionConfig {
                input: InputSpec::Path(entry),
                kind: SourceKind::Opy,
                ..SessionConfig::default()
            },
            Box::new(MappedProvider(SourceCompilation {
                workshop_text: Some(source.to_string()),
                locale: None,
                provenance: SourceProvenance::Mapped(mapped.map),
                diagnostics: Vec::new(),
                source_identity: None,
            })),
        )
        .unwrap();
        for _ in 0..2 {
            let check = session.check();
            assert!(check.ok, "{:?}", check.diagnostics);
            let warning = check
                .diagnostics
                .iter()
                .find(|d| d.code == "source-map-member-unreadable")
                .unwrap();
            assert_eq!(warning.message, expected);
            assert_eq!(warning.stage, crate::Stage::Frontend);
            assert_eq!(warning.severity, crate::Severity::Warning);
        }
    }

    #[test]
    fn source_reads_preserve_member_order_and_errors() {
        let dir = tempfile::tempdir().expect("source directory");
        let files: Vec<_> = (0..32).map(|index| format!("{index}.opy")).collect();
        for (index, file) in files.iter().enumerate().filter(|(index, _)| index % 3 != 0) {
            std::fs::write(dir.path().join(file), format!("source {index} 你好"))
                .expect("source file");
        }
        std::fs::write(dir.path().join(&files[1]), [0xff]).expect("invalid UTF-8");
        let trace = Trace::enabled();
        let root = trace.span("compile");
        let (parsed, sources) = parse_with_sources(&files, dir.path(), &trace, || {
            let _parse = trace.span("parse");
            "parsed"
        });
        assert_eq!(parsed, "parsed");
        assert_eq!(sources.len(), files.len());
        for (index, source) in sources.iter().enumerate() {
            if index % 3 == 0 {
                assert_eq!(source.as_ref().unwrap_err().kind(), io::ErrorKind::NotFound);
            } else if index == 1 {
                assert_eq!(
                    source.as_ref().unwrap_err().kind(),
                    io::ErrorKind::InvalidData
                );
            } else {
                assert_eq!(source.as_ref().unwrap(), &format!("source {index} 你好"));
            }
        }
        drop(root);
        let report = trace.report().unwrap();
        let reads = report
            .spans
            .iter()
            .find(|s| s.name == "source.read")
            .unwrap();
        let parse = report.spans.iter().find(|s| s.name == "parse").unwrap();
        assert_eq!(reads.parent, Some(0));
        assert_eq!(parse.parent, Some(0));
        assert!(reads.facts.contains(&("files", files.len().into())));
        let bytes: usize = sources
            .iter()
            .filter_map(|s| s.as_ref().ok())
            .map(String::len)
            .sum();
        assert!(reads.facts.contains(&("bytes", bytes.into())));
    }
}
