use crate::diag::{Diagnostic, Stage};
use crate::edit::{char_offset_to_utf16, utf16_offset_to_char};

pub(super) fn position_to_column(
    lines: &[&str],
    pos: wright_lpp::Position,
    what: &str,
) -> Result<(u32, u32), Diagnostic> {
    let line_idx = pos.line as usize;
    let Some(line_text) = lines.get(line_idx) else {
        return Err(Diagnostic::error(
            "edit-invalid-range",
            Stage::Discovery,
            format!(
                "the {what} names line {}, but the document has {} lines",
                pos.line,
                lines.len()
            ),
        ));
    };
    let col = utf16_to_char_column(line_text, pos.character).ok_or_else(|| {
        Diagnostic::error(
            "edit-invalid-range",
            Stage::Discovery,
            format!("the {what} names UTF-16 offset {} in line {}, which is not a character boundary of that line", pos.character, pos.line),
        )
    })?;
    Ok(((line_idx as u32) + 1, col))
}

pub(super) fn column_to_position(
    lines: &[&str],
    line: u32,
    column: u32,
) -> Result<wright_lpp::Position, Diagnostic> {
    let line_idx = line.saturating_sub(1) as usize;
    let Some(line_text) = lines.get(line_idx) else {
        return Err(Diagnostic::error(
            "edit-invalid-range",
            Stage::Discovery,
            format!(
                "the edit names line {line}, but the document has {} lines",
                lines.len()
            ),
        ));
    };
    let character = char_column_to_utf16(line_text, column).ok_or_else(|| {
        Diagnostic::error(
            "edit-invalid-range",
            Stage::Discovery,
            format!("the edit names column {column} in line {line}, which is outside that line (line {line} has {} characters)", line_text.chars().count()),
        )
    })?;
    Ok(wright_lpp::Position {
        line: line_idx as u32,
        character,
    })
}

pub(crate) fn utf16_to_char_column(line: &str, units: u32) -> Option<u32> {
    let offset = utf16_offset_to_char(line, units as usize);
    (char_offset_to_utf16(line, offset) == units as usize).then_some(offset as u32 + 1)
}

fn char_column_to_utf16(line: &str, column: u32) -> Option<u32> {
    let offset = column.checked_sub(1)? as usize;
    (offset <= line.chars().count()).then(|| char_offset_to_utf16(line, offset) as u32)
}
