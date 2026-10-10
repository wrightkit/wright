use std::fmt;
use std::path::{Path, PathBuf};

use workshop_rs::program::{MAPPED_TEXT_V1, TEXT_V1};
use workshop_rs::{MappedText, SourceMap};

use crate::diag::{Diagnostic, Origin, Position, Severity, SourceSpan, Stage};
use crate::input::InputTarget;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceLanguage {
    Opy,
}

pub type SourceTargetKind = InputTarget;

impl SourceLanguage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Opy => "opy",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SourceBackend {
    #[default]
    Native,
    Auto,
    Provider,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceTarget {
    pub language: SourceLanguage,
    pub entry: PathBuf,
    pub kind: SourceTargetKind,
    pub cwd: PathBuf,
    pub project_root: Option<PathBuf>,
}

/// How a compiled Workshop artifact relates to the authored source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceProvenance {
    /// The artifact carries no mapping to the authored source.
    Unmapped,
    /// The provider returned `workshop-rs/mapped-text-v1`; the map applies to
    /// the program parsed from the artifact's Workshop text.
    Mapped(SourceMap),
}

impl SourceTarget {
    pub fn new(
        language: SourceLanguage,
        entry: impl Into<PathBuf>,
        cwd: impl Into<PathBuf>,
    ) -> Self {
        let cwd = cwd.into();
        let entry = entry.into();
        let entry = if entry.is_absolute() {
            entry
        } else {
            cwd.join(entry)
        };
        Self {
            language,
            entry,
            kind: InputTarget::File,
            cwd,
            project_root: None,
        }
    }

    pub fn directory(
        language: SourceLanguage,
        directory: impl Into<PathBuf>,
        cwd: impl Into<PathBuf>,
    ) -> Self {
        let mut target = Self::new(language, directory, cwd);
        target.kind = InputTarget::Directory;
        target
    }

    pub fn with_project_root(mut self, project_root: PathBuf) -> Self {
        self.project_root = Some(project_root);
        self
    }

    pub fn entry_path(&self) -> &Path {
        &self.entry
    }

    pub fn is_directory(&self) -> bool {
        self.kind == InputTarget::Directory
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceCompilation {
    pub workshop_text: Option<String>,
    pub locale: Option<String>,
    pub provenance: SourceProvenance,
    pub diagnostics: Vec<Diagnostic>,
    pub source_identity: Option<String>,
}

impl SourceCompilation {
    pub fn success(workshop_text: impl Into<String>) -> Self {
        Self {
            workshop_text: Some(workshop_text.into()),
            locale: None,
            provenance: SourceProvenance::Unmapped,
            diagnostics: Vec::new(),
            source_identity: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceProviderError {
    NotConfigured { language: SourceLanguage },
    Unsupported { message: String },
    Failed { code: String, message: String },
}

/// The diagnostic code prefix carrying a provider's typed LPP refusal
/// (`refusalCode`): `provider-refusal-<refusalCode>`. Wright-originated
/// refusal codes (currently `session-config-changed`, #511) pass through
/// verbatim instead, so they keep the same code as the direct diagnostic
/// path. Other refusals are provider-owned opaque strings (LPP §18.4);
/// namespacing them keeps the refusal class distinguishable without
/// parsing `message`.
const PROVIDER_REFUSAL_CODE_PREFIX: &str = "provider-refusal-";

/// A refusal code Wright itself generated (as opposed to a provider-defined
/// one); it stays verbatim so all of its surfaces report the same code.
fn wright_refusal_code(code: &str) -> bool {
    code == "session-config-changed"
}

/// The stage a `Failed` code reports at. This is the classification the
/// typed `wright_lpp::ProviderError` was carrying when `provider_error`
/// reduced it: deliberate refusals and request-level LPP failures are
/// frontend-stage request outcomes, while transport, environment, and
/// session-state failures are internal. `capability-unavailable` stays a
/// distinct code here; `exit_code_from` maps it to the unsupported exit.
fn failed_stage(code: &str) -> Stage {
    match code {
        "session-config-changed" => Stage::Discovery,
        "provider-refusal" | "refusal" | "capability-unavailable" | "project-load-failed" => {
            Stage::Frontend
        }
        code if code.starts_with(PROVIDER_REFUSAL_CODE_PREFIX) => Stage::Frontend,
        _ => Stage::Internal,
    }
}

impl SourceProviderError {
    pub fn code(&self) -> &str {
        match self {
            Self::NotConfigured { .. } => "source-provider-not-configured",
            Self::Unsupported { .. } => "source-provider-unsupported",
            Self::Failed { code, .. } => code,
        }
    }

    pub fn diagnostic(&self) -> Diagnostic {
        let stage = match self {
            Self::Unsupported { .. } => Stage::Frontend,
            Self::NotConfigured { .. } => Stage::Internal,
            Self::Failed { code, .. } => failed_stage(code),
        };
        Diagnostic::error(self.code(), stage, self.to_string())
    }
}

impl fmt::Display for SourceProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotConfigured { language } => write!(
                f,
                "no source provider is configured for '{}'",
                language.as_str()
            ),
            Self::Unsupported { message } | Self::Failed { message, .. } => f.write_str(message),
        }
    }
}

impl std::error::Error for SourceProviderError {}

pub trait SourceProvider {
    fn language(&self) -> SourceLanguage;
    fn check(&mut self, target: &SourceTarget) -> Result<SourceCompilation, SourceProviderError> {
        self.compile(target)
    }
    fn compile(&mut self, target: &SourceTarget) -> Result<SourceCompilation, SourceProviderError>;
}

pub struct LppSourceProvider {
    provider: Box<dyn wright_lpp::LanguageProvider>,
    locale: Option<String>,
}

impl LppSourceProvider {
    pub fn new(provider: Box<dyn wright_lpp::LanguageProvider>, locale: Option<String>) -> Self {
        Self { provider, locale }
    }

    fn entry(
        &self,
        target: &SourceTarget,
    ) -> Result<wright_lpp::ProjectEntry, SourceProviderError> {
        let uri = url::Url::from_file_path(target.entry_path())
            .map_err(|()| SourceProviderError::Failed {
                code: "provider-invalid-entry".to_string(),
                message: format!(
                    "cannot convert OPY entry '{}' to an absolute file URI",
                    target.entry_path().display()
                ),
            })?
            .to_string();
        Ok(wright_lpp::ProjectEntry {
            uri,
            language_id: SourceLanguage::Opy.as_str().to_string(),
            version: 1,
            kind: if target.is_directory() {
                wright_lpp::ProjectTargetKind::Directory
            } else {
                wright_lpp::ProjectTargetKind::File
            },
        })
    }

    fn project_root_uri(target: &SourceTarget) -> Option<String> {
        target
            .project_root
            .as_deref()
            .and_then(|r| url::Url::from_directory_path(r).ok())
            .map(|u| u.to_string())
    }
}

impl Drop for LppSourceProvider {
    fn drop(&mut self) {
        let _ = self.provider.shutdown();
    }
}

impl SourceProvider for LppSourceProvider {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::Opy
    }

    fn check(&mut self, target: &SourceTarget) -> Result<SourceCompilation, SourceProviderError> {
        let entry = self.entry(target)?;
        let root_uri = Self::project_root_uri(target);
        let res = if target.is_directory() {
            self.provider
                .check_target(&entry, root_uri.as_deref(), self.locale.as_deref())
        } else {
            self.provider
                .check_entry(&entry, root_uri.as_deref(), self.locale.as_deref())
        }
        .map_err(|error| provider_error(error, &request_context("check", target)))?;
        Ok(SourceCompilation {
            workshop_text: None,
            locale: self.locale.clone(),
            provenance: SourceProvenance::Unmapped,
            diagnostics: provider_diagnostics(res.documents, self.locale.as_deref()),
            source_identity: None,
        })
    }

    fn compile(&mut self, target: &SourceTarget) -> Result<SourceCompilation, SourceProviderError> {
        let entry = self.entry(target)?;
        let root_uri = Self::project_root_uri(target);
        // A failed capability query is a provider failure, not a legacy
        // version signal: propagate it instead of silently choosing the
        // unnegotiated compile path (#571).
        let negotiates_artifacts = self
            .provider
            .capabilities()
            .map_err(|error| provider_error(error, &request_context("compile", target)))?
            .protocol_version
            == wright_lpp::LPP_ARTIFACT_NEGOTIATION_PROTOCOL_VERSION;
        let res = if negotiates_artifacts {
            self.provider.compile_target_accepting(
                &entry,
                root_uri.as_deref(),
                self.locale.as_deref(),
                &[MAPPED_TEXT_V1, TEXT_V1],
            )
        } else if target.is_directory() {
            self.provider
                .compile_target(&entry, root_uri.as_deref(), self.locale.as_deref())
        } else {
            self.provider
                .compile_entry(&entry, root_uri.as_deref(), self.locale.as_deref())
        }
        .map_err(|error| provider_error(error, &request_context("compile", target)))?;
        let (workshop_text, provenance) = match res.artifact {
            Some(a) if a.format == TEXT_V1 => (Some(a.content), SourceProvenance::Unmapped),
            Some(a) if negotiates_artifacts && a.format == MAPPED_TEXT_V1 => {
                let mapped = MappedText::from_json(&a.content).map_err(|error| {
                    SourceProviderError::Failed {
                        code: "provider-artifact-format".to_string(),
                        message: format!(
                            "the source provider returned an invalid '{MAPPED_TEXT_V1}' artifact: {error}"
                        ),
                    }
                })?;
                (Some(mapped.text), SourceProvenance::Mapped(mapped.map))
            }
            Some(a) => {
                return Err(SourceProviderError::Failed {
                    code: "provider-artifact-format".to_string(),
                    message: format!(
                        "the source provider returned unsupported artifact format '{}', expected '{TEXT_V1}'",
                        a.format
                    ),
                });
            }
            None => (None, SourceProvenance::Unmapped),
        };
        Ok(SourceCompilation {
            workshop_text,
            locale: self.locale.clone(),
            provenance,
            diagnostics: provider_diagnostics(res.diagnostics, self.locale.as_deref()),
            source_identity: res.source_identity,
        })
    }
}

/// The human-facing context of one provider request: the operation and the
/// entry it ran against (`{operation}` for `{entry}`).
fn request_context(operation: &str, target: &SourceTarget) -> String {
    format!(
        "the source provider {operation} request for '{}' failed",
        target.entry_path().display()
    )
}

/// The code a `wright_lpp::ProviderError` carries across the
/// source-provider boundary. Transport and session failures keep their
/// `ProviderError::code()`; a typed LPP refusal carries its `refusalCode`
/// under the `provider-refusal-` namespace so consumers can still classify
/// the refusal without parsing `message` (LPP §18.4 makes refusal codes
/// machine-readable). A Wright-originated refusal code stays verbatim.
pub(crate) fn provider_failure_code(error: &wright_lpp::ProviderError) -> String {
    match error.refusal_code() {
        Some(code) if wright_refusal_code(code) => code.to_string(),
        Some(code) => format!("{PROVIDER_REFUSAL_CODE_PREFIX}{code}"),
        // A `refusal` kind whose details carry no `refusalCode` is still a
        // deliberate decline, not a generic `refusal`-coded session error.
        None if error.code() == "refusal" => "provider-refusal".to_string(),
        None => error.code().to_string(),
    }
}

/// Classify one provider failure into the stage its diagnostic reports at:
/// the same mapping `SourceProviderError::diagnostic` applies to the code
/// `provider_failure_code` produced. Keeping this on the typed error lets
/// every `ProviderError` consumer share the source-provider classification
/// rules instead of re-deriving them.
pub(crate) fn provider_failure_stage(error: &wright_lpp::ProviderError) -> Stage {
    failed_stage(&provider_failure_code(error))
}

/// Reduce a typed `wright_lpp::ProviderError` to the source-provider error
/// contract, preserving the machine-usable classification in `code` and the
/// failing operation/entry context in the human message. Classification
/// happens here, while the typed error is still available; downstream
/// consumers only see the resulting code and stage.
pub(crate) fn provider_error(
    error: wright_lpp::ProviderError,
    context: &str,
) -> SourceProviderError {
    SourceProviderError::Failed {
        code: provider_failure_code(&error),
        message: format!("{context}: {error}"),
    }
}

fn provider_diagnostics(
    documents: Vec<wright_lpp::DocumentDiagnostics>,
    locale: Option<&str>,
) -> Vec<Diagnostic> {
    documents
        .into_iter()
        .enumerate()
        .flat_map(|(file, doc)| {
            let path = provider_uri_path(&doc.uri);
            doc.diagnostics.into_iter().map(move |d| {
                let severity = match d.severity {
                    wright_lpp::DiagnosticSeverity::Error => Severity::Error,
                    wright_lpp::DiagnosticSeverity::Warning => Severity::Warning,
                    _ => Severity::Info,
                };
                Diagnostic {
                    code: d.code.unwrap_or_else(|| "provider-diagnostic".to_string()),
                    stage: Stage::Frontend,
                    severity,
                    message: d.message,
                    status: None,
                    span: Some(SourceSpan {
                        file,
                        path: path.clone(),
                        start: provider_position(d.range.start),
                        end: provider_position(d.range.end),
                    }),
                    source: Some(Origin {
                        kind: SourceLanguage::Opy.as_str().to_string(),
                        locale: locale.map(str::to_owned),
                    }),
                }
            })
        })
        .collect()
}

pub(crate) fn provider_uri_path(uri: &str) -> String {
    url::Url::parse(uri)
        .ok()
        .and_then(|u| u.to_file_path().ok())
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| uri.to_string())
}

/// The identity/version precondition view a defaulted `sources` mirrors
/// (#548): every document's own text under its URI.
pub(crate) fn document_sources(
    documents: &wright_lpp::DocumentSet,
) -> std::collections::BTreeMap<String, String> {
    documents
        .values()
        .map(|document| (document.uri.clone(), document.text.clone()))
        .collect()
}

/// The provider document set a mutation runs against (#548, #583): the
/// loaded project's members, each read from disk at `version` 0 and keyed
/// by `file://` URI, so the provider sees the same project the session
/// serves. A member that is not a readable disk file is a structured
/// refusal, never a silently partial set.
pub(crate) fn provider_document_set(
    members: &[String],
    root: &Path,
    language_id: &str,
) -> Result<wright_lpp::DocumentSet, wright_analyzer::service::ErrorInfo> {
    let mut set = wright_lpp::DocumentSet::new();
    for member in members {
        let (uri, path) = provider_member(member, root)?;
        let text = std::fs::read_to_string(&path).map_err(|error| {
            wright_analyzer::service::ErrorInfo {
                code: "provider-document-unreadable".to_string(),
                message: format!(
                    "cannot read loaded project member '{}' for the provider document set: {error}",
                    path.display()
                ),
            }
        })?;
        set.insert(
            uri.clone(),
            wright_lpp::Document {
                uri,
                language_id: language_id.to_string(),
                version: 0,
                text,
            },
        );
    }
    Ok(set)
}

/// The `project_root` URI a provider request carries for a resolved input
/// root — the same directory-URI spelling `LppSourceProvider` sends.
pub(crate) fn provider_project_root(root: &Path) -> Option<String> {
    url::Url::from_directory_path(root)
        .ok()
        .map(|u| u.to_string())
}

/// A loaded project member as `(file:// URI, absolute disk path)` (#548): a
/// URI spelling keeps its identity and converts back to its path; a bare
/// path absolutizes against the session's project root — the root the
/// provider was given — and converts to a `file://` URI. A member that is
/// not a disk file refuses — the defaulted document set covers only sources
/// the session read from disk.
pub(crate) fn provider_member(
    member: &str,
    root: &std::path::Path,
) -> Result<(String, std::path::PathBuf), wright_analyzer::service::ErrorInfo> {
    // A Windows drive-absolute spelling (`C:\...`, `C:/...`) must be handled
    // before URI parsing: `url::Url::parse` accepts it as a URI whose scheme
    // is the drive letter, and `to_file_path` then refuses. The provider
    // emits these spellings on Windows hosts; treat them as disk paths on
    // every host so the refusal contract stays about URI kind, not host OS.
    if let Some(pair) = windows_drive_member(member) {
        return Ok(pair);
    }
    if let Ok(url) = url::Url::parse(member) {
        let path = url
            .to_file_path()
            .map_err(|()| wright_analyzer::service::ErrorInfo {
                code: "provider-document-uri".to_string(),
                message: format!(
                    "loaded project member '{member}' is not a disk file and cannot serve the \
                     provider document set"
                ),
            })?;
        return Ok((url.to_string(), path));
    }
    let path = {
        let path = std::path::PathBuf::from(member);
        if path.is_absolute() {
            path
        } else {
            root.join(path)
        }
    };
    url::Url::from_file_path(&path)
        .map(|url| (url.to_string(), path))
        .map_err(|()| wright_analyzer::service::ErrorInfo {
            code: "provider-document-uri".to_string(),
            message: format!(
                "loaded project member '{member}' cannot be expressed as a file:// URI"
            ),
        })
}

/// `C:\dir\file.opy` or `C:/dir/file.opy` → `(file:///C:/dir/file.opy, path)`.
/// The path is percent-encoded before parsing so literal `#`, `%`, `?`, and
/// friends are not reinterpreted as URL syntax — `Url::parse` alone would
/// treat `#` as a fragment delimiter and `%xx` as existing escapes.
fn windows_drive_member(member: &str) -> Option<(String, std::path::PathBuf)> {
    const PATH_ENCODE: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
        .add(b' ')
        .add(b'"')
        .add(b'#')
        .add(b'%')
        .add(b'<')
        .add(b'>')
        .add(b'?')
        .add(b'`')
        .add(b'{')
        .add(b'|')
        .add(b'}')
        .add(b'^');
    let bytes = member.as_bytes();
    let drive_absolute = bytes.len() > 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    if !drive_absolute {
        return None;
    }
    let path = std::path::PathBuf::from(member);
    let normalized = member.replace('\\', "/");
    let encoded = percent_encoding::utf8_percent_encode(&normalized, PATH_ENCODE);
    let url = url::Url::parse(&format!("file:///{encoded}")).ok()?;
    Some((url.to_string(), path))
}

fn provider_position(pos: wright_lpp::Position) -> Position {
    Position {
        line: pos.line.saturating_add(1),
        col: pos.character.saturating_add(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wright_lpp::{LppErrorKind, ProviderError};

    #[test]
    fn windows_drive_member_is_a_disk_path_not_a_uri_scheme() {
        // `url::Url::parse` accepts `C:\...` as a URI with scheme `c`; the
        // provider member helper must classify it as a filesystem path first.
        let (uri, path) = provider_member("C:\\project\\main.opy", Path::new("/cwd"))
            .expect("windows drive member resolves");
        assert_eq!(uri, "file:///C:/project/main.opy");
        assert_eq!(path, PathBuf::from("C:\\project\\main.opy"));

        let (uri, _) = provider_member("D:/work/lib.opy", Path::new("/cwd"))
            .expect("forward-slash drive member resolves");
        assert_eq!(uri, "file:///D:/work/lib.opy");

        // Literal `#`/`%`/`?` in the path are encoded, not parsed as URL
        // syntax, and decode back to the same spelling.
        for (member, uri) in [
            ("C:\\project#1\\main.opy", "file:///C:/project%231/main.opy"),
            (
                "C:\\project%20name\\main.opy",
                "file:///C:/project%2520name/main.opy",
            ),
        ] {
            let (produced, _) =
                provider_member(member, Path::new("/cwd")).expect("member resolves");
            assert_eq!(produced, uri);
            let url = url::Url::parse(&produced).expect("uri parses");
            let decoded = percent_encoding::percent_decode_str(url.path())
                .decode_utf8()
                .expect("utf8");
            assert_eq!(decoded, format!("/{}", member.replace('\\', "/")));
        }
    }

    #[test]
    fn file_uri_member_keeps_its_spelling() {
        let (uri, path) =
            provider_member("file:///project/main.opy", Path::new("/cwd")).expect("file URI");
        assert_eq!(uri, "file:///project/main.opy");
        assert_eq!(path, PathBuf::from("/project/main.opy"));
    }

    #[test]
    fn non_file_uri_member_refuses() {
        let error = provider_member("untitled:main.opy", Path::new("/cwd"))
            .expect_err("non-file scheme refuses");
        assert_eq!(error.code, "provider-document-uri");
    }

    #[test]
    fn relative_member_resolves_against_the_project_root() {
        let (uri, path) = provider_member("src/lib.opy", Path::new("/project"))
            .expect("relative member resolves");
        assert_eq!(path, PathBuf::from("/project/src/lib.opy"));
        assert_eq!(uri, "file:///project/src/lib.opy");
    }

    #[test]
    fn relative_entry_is_resolved_from_the_invocation_directory() {
        let target = SourceTarget::new(SourceLanguage::Opy, "src/main.opy", "/project");
        assert_eq!(target.entry, PathBuf::from("/project/src/main.opy"));
        assert_eq!(target.cwd, PathBuf::from("/project"));
    }

    #[test]
    fn provider_failures_have_a_distinct_structured_code() {
        let diagnostic = SourceProviderError::Failed {
            code: "provider-exited".to_string(),
            message: "provider exited".to_string(),
        }
        .diagnostic();
        assert_eq!(diagnostic.code, "provider-exited");
        assert_eq!(diagnostic.stage, Stage::Internal);
        assert!(diagnostic.span.is_none());
    }

    /// #569: each typed `ProviderError` keeps a machine-usable
    /// classification across the source-provider boundary instead of
    /// collapsing into one generic internal failure.
    #[test]
    fn provider_error_classification_distinguishes_typed_causes() {
        let cases: &[(ProviderError, &str, Stage)] = &[
            (
                ProviderError::Timeout {
                    method: "lpp/compile".to_string(),
                    duration: std::time::Duration::from_secs(5),
                },
                "provider-timeout",
                Stage::Internal,
            ),
            (
                ProviderError::Exited {
                    status: Some(1),
                    message: "the provider exited".to_string(),
                },
                "provider-exited",
                Stage::Internal,
            ),
            (
                ProviderError::Malformed {
                    detail: "missing id".to_string(),
                },
                "provider-malformed",
                Stage::Internal,
            ),
            (
                ProviderError::lpp(
                    LppErrorKind::CapabilityUnavailable,
                    serde_json::json!({"capability": "projectLoading", "method": "lpp/check"}),
                    "capability not negotiated",
                ),
                "capability-unavailable",
                Stage::Frontend,
            ),
            (
                ProviderError::lpp(
                    LppErrorKind::ProjectLoadFailed,
                    serde_json::json!({"entryUri": "file:///p/main.opy", "reason": "unreadable"}),
                    "could not load the project",
                ),
                "project-load-failed",
                Stage::Frontend,
            ),
            (
                ProviderError::lpp(
                    LppErrorKind::Refusal,
                    serde_json::json!({"refusalCode": "compile.tooLarge"}),
                    "the project is too large",
                ),
                "provider-refusal-compile.tooLarge",
                Stage::Frontend,
            ),
            (
                ProviderError::lpp(
                    LppErrorKind::Refusal,
                    serde_json::json!({"refusalCode": "session-config-changed"}),
                    "the session configuration changed",
                ),
                "session-config-changed",
                Stage::Discovery,
            ),
        ];
        for (error, code, stage) in cases {
            let mapped = provider_error(error.clone(), "the request failed");
            assert_eq!(
                mapped.code(),
                *code,
                "{error:?} kept the wrong classification code"
            );
            assert_eq!(
                mapped.diagnostic().stage,
                *stage,
                "{error:?} reports at the wrong stage"
            );
            assert!(
                mapped.to_string().starts_with("the request failed: "),
                "{error:?} lost its operation context"
            );
        }
    }

    /// #569: a refusal without a `refusalCode` detail still classifies as a
    /// refusal rather than an internal failure.
    #[test]
    fn refusal_without_a_detail_code_stays_a_refusal() {
        let mapped = provider_error(
            ProviderError::lpp(LppErrorKind::Refusal, serde_json::json!({}), "declined"),
            "the request failed",
        );
        assert_eq!(mapped.code(), "provider-refusal");
        assert_eq!(mapped.diagnostic().stage, Stage::Frontend);
    }

    /// #569: timeout method/duration and the typed refusal reason stay in
    /// the message; nothing on this path reads `message` back for decisions.
    #[test]
    fn provider_error_message_keeps_the_typed_detail_in_text() {
        let mapped = provider_error(
            ProviderError::Timeout {
                method: "lpp/compile".to_string(),
                duration: std::time::Duration::from_millis(2500),
            },
            "the compile request failed",
        );
        let message = mapped.to_string();
        assert!(message.contains("'lpp/compile'"), "{message}");
        assert!(message.contains("2500ms"), "{message}");
    }
}
