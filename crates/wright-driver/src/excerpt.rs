//! The bounded one-line source excerpt a `refs` reference carries (#1099).

use serde_json::Value;
use workshop_rs::source::{FileId, Position, Span};

use crate::Loaded;
use crate::session::Provenance;

/// The most source characters an excerpt keeps; clipped ends add a `…`.
pub(crate) const EXCERPT_WIDTH: usize = 140;
const CLIP: char = '…';

/// Add an `excerpt` to each reference that has a span into authored source.
/// A reference without such a span, or from an unmapped provider artifact,
/// gets none.
pub(crate) fn attach_excerpts(references: &mut Value, name: &str, loaded: &Loaded) {
    if loaded.provenance == Provenance::Unmapped {
        return;
    }
    let Some(references) = references.as_array_mut() else {
        return;
    };
    for reference in references {
        if let Some(text) = reference_excerpt(&reference["span"], name, loaded) {
            reference["excerpt"] = Value::String(text);
        }
    }
}

fn reference_excerpt(span: &Value, name: &str, loaded: &Loaded) -> Option<String> {
    let position = |key: &str| {
        Some(Position::new(
            u32::try_from(span[key]["line"].as_u64()?).ok()?,
            u32::try_from(span[key]["col"].as_u64()?).ok()?,
        ))
    };
    let file = FileId::from_index(usize::try_from(span["file"].as_u64()?).ok()?);
    let span = Span::new(file, position("start")?, position("end")?);
    let document = loaded.program.source(file)?;
    let range = document.byte_range(span)?;
    Some(excerpt(&document.text()[range], name))
}

/// Join the span's lines into one, then keep at most [`EXCERPT_WIDTH`]
/// characters: all of it when it fits, else a window centered on the first
/// occurrence of `name`, or starting at the span start when `name` is absent.
fn excerpt(span_text: &str, name: &str) -> String {
    let flat = span_text
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ");
    let chars: Vec<char> = flat.trim().chars().collect();
    if chars.len() <= EXCERPT_WIDTH {
        return chars.into_iter().collect();
    }
    let needle: Vec<char> = name.chars().collect();
    let start = find(&chars, &needle).map_or(0, |at| {
        let margin = EXCERPT_WIDTH.saturating_sub(needle.len()) / 2;
        at.saturating_sub(margin).min(chars.len() - EXCERPT_WIDTH)
    });
    let end = start + EXCERPT_WIDTH;
    let mut out = String::new();
    if start > 0 {
        out.push(CLIP);
    }
    out.extend(&chars[start..end]);
    if end < chars.len() {
        out.push(CLIP);
    }
    out
}

fn find(haystack: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_span_is_kept_whole_on_one_line() {
        assert_eq!(excerpt("Set A(\n    b,\n    c);", "b"), "Set A( b, c);");
    }

    #[test]
    fn long_span_centers_on_first_occurrence_of_the_name() {
        let text = format!("{}needle{}needle", "a".repeat(300), "b".repeat(300));
        let out = excerpt(&text, "needle");
        assert!(out.starts_with(CLIP) && out.ends_with(CLIP));
        assert_eq!(out.chars().count(), EXCERPT_WIDTH + 2);
        assert_eq!(out.matches("needle").count(), 1);
        assert!(out.contains(&format!("a{}needle{}b", "a".repeat(66), "b".repeat(66))[..]));
    }

    #[test]
    fn long_span_without_the_name_starts_at_the_span_start() {
        let out = excerpt(&"x".repeat(500), "missing");
        assert_eq!(out, format!("{}{CLIP}", "x".repeat(EXCERPT_WIDTH)));
    }

    #[test]
    fn window_clamps_at_both_ends() {
        let head = format!("needle{}", "x".repeat(500));
        assert_eq!(&excerpt(&head, "needle")[..6], "needle");
        let tail = format!("{}needle", "x".repeat(500));
        assert!(excerpt(&tail, "needle").ends_with("needle"));
    }
}
