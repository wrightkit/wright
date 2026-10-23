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
