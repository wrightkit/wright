use crate::diag::Position;

pub(super) fn provider_position(pos: wright_lpp::Position, lines: &[String]) -> Position {
    let col = lines
        .get(pos.line as usize)
        .and_then(|line| crate::provider_edit::utf16_to_char_column(line, pos.character))
        .unwrap_or_else(|| pos.character.saturating_add(1));
    Position {
        line: pos.line.saturating_add(1),
        col,
    }
}

/// Resolve a provider's member spelling — a `file://` URI, a Windows
/// drive-absolute path, or a project-relative member — to its canonical
/// URI and disk path.
pub(crate) fn provider_member(
    member: &str,
    base: &std::path::Path,
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
        let path = url.to_file_path().map_err(|()| {
            wright_analyzer::service::ErrorInfo::new(
                "provider-document-uri",
                format!(
                    "loaded project member '{}' is not a disk file and cannot serve the \
                         provider document set",
                    member
                ),
            )
        })?;
        return Ok((url.to_string(), path));
    }
    let path = base.join(member);
    url::Url::from_file_path(&path)
        .map(|url| (url.to_string(), path))
        .map_err(|()| {
            wright_analyzer::service::ErrorInfo::new(
                "provider-document-uri",
                format!(
                    "loaded project member '{}' cannot be expressed as a file:// URI",
                    member
                ),
            )
        })
}

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
