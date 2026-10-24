pub(super) fn position_byte(lines: &[&str], line: u32, col: u32) -> usize {
    lines[..line as usize - 1]
        .iter()
        .map(|line| line.len() + 1)
        .sum::<usize>()
        + char_col(lines[line as usize - 1], col)
}

/// Convert a UTF-16 column to a Rust character offset, rounding up within a surrogate pair.
pub fn utf16_offset_to_char(line: &str, utf16_offset: usize) -> usize {
    let mut chars = 0usize;
    let mut utf16 = 0usize;
    for c in line.chars() {
        if utf16 >= utf16_offset {
            break;
        }
        utf16 += c.len_utf16();
        chars += 1;
    }
    chars
}

/// Convert a Rust character offset to a UTF-16 column, clamping at the line end.
pub fn char_offset_to_utf16(line: &str, char_offset: usize) -> usize {
    line.chars().take(char_offset).map(char::len_utf16).sum()
}

pub(super) fn char_col(line: &str, col: u32) -> usize {
    let skip = col.saturating_sub(1) as usize;
    line.char_indices()
        .nth(skip)
        .map(|(off, _)| off)
        .unwrap_or(line.len())
}
