//! Workshop client-language detection and explicit locale override.
//!
//! Detection scores each catalog-declared locale by how many distinct
//! localized builtin/struct/enum aliases appear in the input text. With the
//! v0.2 en-US catalog this confirms English Workshop input; adding locales is
//! a data-pipeline change that makes cross-locale ranking meaningful.
//! Ambiguous or low-confidence input fails explicitly rather than selecting
//! arbitrarily, and an explicit locale always bypasses detection.

use crate::catalog::{Catalog, Locale};
use crate::error::{Result, WorkshopError};

/// A language-detection result with ranked evidence.
#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    /// The best-matching locale.
    pub locale: Locale,
    /// Confidence in `[0, 1)`; grows with the number of distinct matches.
    pub confidence: f64,
    /// Distinct catalog aliases found for the best locale.
    pub matches: usize,
    /// Every candidate locale with its match count, ranked descending.
    pub candidates: Vec<(Locale, usize)>,
}

/// The minimum distinct-match count required to trust a detection.
pub const MIN_MATCHES: usize = 2;

/// Detect the Workshop client language of the input.
pub fn detect(input: &str, catalog: &Catalog) -> Detection {
    let mut candidates: Vec<(Locale, usize)> = catalog
        .locales()
        .iter()
        .map(|locale| {
            let matches = locale_alias_matches(input, catalog, locale);
            (locale.clone(), matches)
        })
        .collect();
    candidates.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    let (locale, matches) = candidates
        .first()
        .cloned()
        .unwrap_or_else(|| (Locale::new("en-US"), 0));
    let confidence = matches as f64 / (matches as f64 + 1.0);
    Detection {
        locale,
        confidence,
        matches,
        candidates,
    }
}

/// Resolve a locale for parsing: an explicit override always wins; otherwise
/// auto-detect and require a confident, unambiguous match.
pub fn resolve_locale(
    input: &str,
    catalog: &Catalog,
    override_locale: Option<&Locale>,
) -> Result<Locale> {
    if let Some(locale) = override_locale {
        if !catalog.supports(locale) {
            return Err(WorkshopError::Unknown {
                kind: "locale",
                spelling: locale.to_string(),
                locale: locale.clone(),
                span: None,
            });
        }
        return Ok(locale.clone());
    }
    let detection = detect(input, catalog);
    if detection.matches == 0 {
        return Err(WorkshopError::Unknown {
            kind: "language",
            spelling: "<none>".to_string(),
            locale: detection.locale,
            span: None,
        });
    }
    if detection.matches < MIN_MATCHES {
        return Err(WorkshopError::Unsupported {
            message: format!(
                "insufficient evidence to detect the Workshop client language ({} distinct match(es))",
                detection.matches
            ),
            span: None,
        });
    }
    if detection.candidates.len() > 1
        && detection.candidates[0].1 == detection.candidates[1].1
        && detection.candidates[0].1 > 0
    {
        return Err(WorkshopError::Unsupported {
            message: "ambiguous Workshop client language: multiple locales tie".to_string(),
            span: None,
        });
    }
    Ok(detection.locale)
}

/// Count distinct catalog aliases of `locale` that appear in the input.
fn locale_alias_matches(input: &str, catalog: &Catalog, locale: &Locale) -> usize {
    let mut matches = 0usize;
    for kind in [
        crate::catalog::Kind::Structural,
        crate::catalog::Kind::Action,
        crate::catalog::Kind::Value,
        crate::catalog::Kind::Event,
        crate::catalog::Kind::Operator,
    ] {
        for entry in catalog.entries_of(kind) {
            if let Some(spelling) = entry.spelling(locale) {
                if contains_word(input, spelling) {
                    matches += 1;
                }
            }
        }
    }
    // Enum member spellings (e.g. "Grapple Beam", "Ignore Condition").
    for domain in catalog.enum_domains() {
        for member in &domain.members {
            if let Some(spelling) = member.spelling(locale) {
                if contains_word(input, spelling) {
                    matches += 1;
                }
            }
        }
    }
    matches
}

/// Whether `needle` appears in `haystack` bounded by non-word characters.
fn contains_word(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let mut search_from = 0;
    while let Some(index) = haystack[search_from..].find(needle) {
        let start = search_from + index;
        let end = start + needle.len();
        let before_ok = start == 0
            || !haystack[..start]
                .chars()
                .next_back()
                .is_some_and(is_word_char);
        let after_ok =
            end >= haystack.len() || !haystack[end..].chars().next().is_some_and(is_word_char);
        if before_ok && after_ok {
            return true;
        }
        search_from = start + 1;
    }
    false
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '-'
}
