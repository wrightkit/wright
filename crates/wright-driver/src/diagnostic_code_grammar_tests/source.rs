//! Source-reading machinery shared by the contract censuses: comment and
//! literal masking (so structural scans never match inside a literal),
//! bracket and argument matching, the enclosing-function lookup, and the
//! `#[cfg(test)]` blanking that keeps the census on production code.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

// ===== source machinery =====

/// The crate source root (`crates/wright-driver/src`).
pub(super) fn crate_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// The pinned `workshop-rs` source root, located through cargo so the census
/// reads the exact resolved revision, path or registry alike.
pub(super) fn workshop_rs_src() -> PathBuf {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--offline"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata must run to enumerate engine error kinds");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata output is JSON");
    let manifest = metadata["packages"]
        .as_array()
        .expect("cargo metadata has a packages array")
        .iter()
        .find(|package| package["name"] == "workshop-rs")
        .and_then(|package| package["manifest_path"].as_str())
        .expect("the resolved graph contains workshop-rs");
    PathBuf::from(manifest)
        .parent()
        .expect("the package manifest has a parent directory")
        .join("src")
}

pub(super) fn read_rs_tree(dir: &Path) -> BTreeMap<PathBuf, String> {
    let mut files = BTreeMap::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in
            std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.insert(
                    path.clone(),
                    std::fs::read_to_string(&path)
                        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display())),
                );
            }
        }
    }
    files
}

/// A byte copy of `src` with the contents of comments, string/char literals,
/// and raw strings replaced by spaces (delimiters kept, positions and
/// newlines preserved), so structural scanning never matches inside a
/// literal.
pub(super) fn mask_rust(src: &str) -> Vec<u8> {
    let bytes = src.as_bytes();
    let mut mask = bytes.to_vec();
    let mut blank = |range: std::ops::Range<usize>| {
        for pos in range {
            if mask[pos] != b'\n' {
                mask[pos] = b' ';
            }
        }
    };
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                let end = src[i..].find('\n').map(|n| i + n).unwrap_or(bytes.len());
                blank(i..end);
                i = end;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                let mut end = i + 2;
                while end < bytes.len()
                    && !(bytes[end] == b'*' && bytes.get(end + 1) == Some(&b'/'))
                {
                    end += 1;
                }
                let end = (end + 2).min(bytes.len());
                blank(i..end);
                i = end;
            }
            b'r' => {
                // Possibly a raw string: r"..." or r#"..."#…
                let mut j = i + 1;
                while bytes.get(j) == Some(&b'#') {
                    j += 1;
                }
                if bytes.get(j) != Some(&b'"') {
                    i += 1;
                    continue;
                }
                let hashes = j - i - 1;
                let close = format!("\"{}", "#".repeat(hashes));
                let end = src[j + 1..]
                    .find(&close)
                    .map(|n| j + 1 + n + close.len())
                    .unwrap_or(bytes.len());
                blank(i..end);
                i = end;
            }
            b'"' => {
                let mut end = i + 1;
                while end < bytes.len() && bytes[end] != b'"' {
                    if bytes[end] == b'\\' {
                        end += 1;
                    }
                    end += 1;
                }
                blank(i + 1..end.min(bytes.len()));
                i = (end + 1).min(bytes.len());
            }
            b'\'' if bytes.get(i + 1) == Some(&b'\\') => {
                // An escaped char literal (`'\n'`, `'\''`, `'\u{1F600}'`):
                // scan to the closing quote. Lifetimes never carry `\`.
                let mut end = i + 2;
                if bytes.get(end) == Some(&b'\'') {
                    end += 1; // `'\''` — the escaped char is a quote itself
                }
                while end < bytes.len()
                    && bytes[end] != b'\''
                    && bytes[end] != b'\n'
                    && end - i <= 12
                {
                    end += 1;
                }
                if end < bytes.len() && bytes[end] == b'\'' && end > i + 2 {
                    blank(i + 1..end);
                    i = end + 1;
                } else {
                    i += 1;
                }
            }
            b'\'' => {
                // A bare char literal is exactly one char between quotes —
                // `'x'`, `'é'` — so a `'a`/`'static` lifetime, whose next
                // quote is never one char away, stays unmasked.
                let len = match bytes.get(i + 1) {
                    Some(b) if b.is_ascii() => 1,
                    Some(b) if (0xC0..=0xDFu8).contains(b) => 2,
                    Some(b) if (0xE0..=0xEFu8).contains(b) => 3,
                    Some(b) if (0xF0..=0xF7u8).contains(b) => 4,
                    _ => 0,
                };
                if len > 0 && bytes.get(i + 1 + len) == Some(&b'\'') {
                    blank(i + 1..i + 1 + len);
                    i += len + 2;
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    mask
}

pub(super) fn as_str(mask: &[u8], range: std::ops::Range<usize>) -> &str {
    std::str::from_utf8(&mask[range]).expect("masked source stays valid UTF-8")
}

/// Blank every `#[cfg(test)]`-gated item (positions preserved) so only
/// production code is scanned.
pub(super) fn blank_cfg_test_items(mask: &[u8]) -> Vec<u8> {
    let mut out = mask.to_vec();
    let text = String::from_utf8_lossy(mask);
    let mut search_from = 0;
    while let Some(at) = text[search_from..]
        .find("#[cfg(test)]")
        .map(|n| search_from + n)
    {
        let mut i = at + "#[cfg(test)]".len();
        // The gated item may carry further attributes; skip `#[…]` groups
        // and whitespace to the item itself.
        loop {
            while i < mask.len() && (mask[i] as char).is_ascii_whitespace() {
                i += 1;
            }
            if mask[i..].starts_with(b"#[") {
                i = match_bracket(mask, i + 1)
                    .map(|end| end + 1)
                    .unwrap_or(i + 2);
            } else {
                break;
            }
        }
        // The item ends at the first top-level `;` or at the balanced `}`
        // closing its first `{`.
        let mut depth = 0i32;
        let mut end = i;
        while end < mask.len() {
            match mask[end] {
                b';' if depth == 0 => {
                    end += 1;
                    break;
                }
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        end += 1;
                        break;
                    }
                }
                _ => {}
            }
            end += 1;
        }
        for byte in &mut out[at..end.min(mask.len())] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
        search_from = end;
    }
    out
}

/// The index of the bracket matching the `(`, `[`, or `{` at `open`.
pub(super) fn match_bracket(mask: &[u8], open: usize) -> Option<usize> {
    let (open_ch, close_ch) = match mask.get(open) {
        Some(b'(') => (b'(', b')'),
        Some(b'[') => (b'[', b']'),
        Some(b'{') => (b'{', b'}'),
        _ => return None,
    };
    let mut depth = 0i32;
    for (i, &b) in mask.iter().enumerate().skip(open) {
        if b == open_ch {
            depth += 1;
        } else if b == close_ch {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// The (start, end) byte ranges of the top-level arguments of the call
/// whose `(` sits at `open`.
pub(super) fn call_args(mask: &[u8], open: usize) -> Vec<(usize, usize)> {
    let mut args = Vec::new();
    let mut depth = 0i32;
    let mut arg_start = open + 1;
    for (i, &b) in mask.iter().enumerate().skip(open + 1) {
        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                if depth == 0 {
                    args.push((arg_start, i));
                    return args;
                }
                depth -= 1;
            }
            b',' if depth == 0 => {
                args.push((arg_start, i));
                arg_start = i + 1;
            }
            _ => {}
        }
    }
    args
}

/// The (name, params) of the nearest `fn` whose body encloses `pos`; params
/// are (name, type) pairs with the receiver omitted.
pub(super) fn enclosing_fn(mask: &[u8], pos: usize) -> Option<(String, Vec<(String, String)>)> {
    let text = String::from_utf8_lossy(mask);
    let mut search_end = pos;
    loop {
        let fn_at = text[..search_end].rfind("fn ")?;
        let name_start = fn_at + 3;
        let name_end = text[name_start..]
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .map(|n| name_start + n)?;
        let paren = text[name_end..].find('(').map(|n| name_end + n)?;
        let params_end = match_bracket(mask, paren)?;
        let body = text[params_end..].find('{').map(|n| params_end + n)?;
        let body_end = match_bracket(mask, body)?;
        if pos > body && pos < body_end {
            let params = call_args(mask, paren)
                .iter()
                .filter_map(|(s, e)| {
                    let param = text[*s..*e].trim();
                    if param.is_empty()
                        || param == "self"
                        || param == "&self"
                        || param == "&mut self"
                    {
                        return None;
                    }
                    let (name, ty) = param.split_once(':')?;
                    Some((
                        name.trim().trim_start_matches("mut ").to_string(),
                        ty.trim().to_string(),
                    ))
                })
                .collect();
            return Some((text[name_start..name_end].to_string(), params));
        }
        search_end = fn_at;
    }
}

/// The `(start, end)` byte range of the `{…}` body of the `fn` enclosing
/// `pos` — the same walk `enclosing_fn` performs, returning the span.
pub(super) fn enclosing_fn_body(mask: &[u8], pos: usize) -> Option<(usize, usize)> {
    let text = String::from_utf8_lossy(mask);
    let mut search_end = pos;
    loop {
        let fn_at = text[..search_end].rfind("fn ")?;
        let name_start = fn_at + 3;
        let name_end = text[name_start..]
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .map(|n| name_start + n)?;
        let paren = text[name_end..].find('(').map(|n| name_end + n)?;
        let params_end = match_bracket(mask, paren)?;
        let body = text[params_end..].find('{').map(|n| params_end + n)?;
        let body_end = match_bracket(mask, body)?;
        if pos > body && pos < body_end {
            return Some((body, body_end));
        }
        search_end = fn_at;
    }
}

pub(super) fn line_of(src: &str, pos: usize) -> usize {
    src[..pos].matches('\n').count() + 1
}
