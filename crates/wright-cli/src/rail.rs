//! The terminal report rail (#718): one continuous line from the verdict to
//! the footer that groups a report, with a blank rail line between
//! independent blocks.
//!
//! Renderers print through this module's `println!`/`eprintln!`. While a
//! report is being captured the lines are collected instead of written, so
//! the frame can see the whole report — which line opens the rail, which
//! closes it — before anything reaches the terminal. Outside a capture they
//! print exactly as the standard macros do.

use std::cell::RefCell;

/// Opens an item block (envelope diagnostic, lint finding, `inspect cfg`
/// block, ...) on the rail.
pub(crate) const BRANCH: &str = "\u{1}";
/// Opens the closing footer line.
pub(crate) const END: &str = "\u{2}";
/// Prefixes a child line that is already laid out relative to the item
/// text; the frame supplies the rail and keeps the rest verbatim.
pub(crate) const CHILD: &str = "\u{3}";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Stream {
    Out,
    Err,
}

pub(crate) type Line = (Stream, String);

thread_local! {
    static SINK: RefCell<Option<Vec<Line>>> = const { RefCell::new(None) };
}

pub(crate) fn line(stream: Stream, text: String) {
    let captured = SINK.with(|sink| match sink.borrow_mut().as_mut() {
        Some(lines) => {
            lines.extend(text.split('\n').map(|part| (stream, part.to_string())));
            true
        }
        None => false,
    });
    if !captured {
        match stream {
            Stream::Out => std::println!("{text}"),
            Stream::Err => std::eprintln!("{text}"),
        }
    }
}

/// Runs `render` and returns the lines it printed instead of writing them.
pub(crate) fn capture(render: impl FnOnce()) -> Vec<Line> {
    SINK.with(|sink| *sink.borrow_mut() = Some(Vec::new()));
    render();
    SINK.with(|sink| sink.borrow_mut().take().unwrap_or_default())
}

pub(crate) fn emit(lines: Vec<Line>) {
    for (stream, text) in lines {
        line(stream, text);
    }
}

macro_rules! println {
    () => { $crate::rail::line($crate::rail::Stream::Out, ::std::string::String::new()) };
    ($($arg:tt)*) => { $crate::rail::line($crate::rail::Stream::Out, ::std::format!($($arg)*)) };
}
macro_rules! eprintln {
    () => { $crate::rail::line($crate::rail::Stream::Err, ::std::string::String::new()) };
    ($($arg:tt)*) => { $crate::rail::line($crate::rail::Stream::Err, ::std::format!($($arg)*)) };
}
pub(crate) use {eprintln, println};

struct Framed {
    stream: Stream,
    glyph: &'static str,
    text: String,
}

fn blank(stream: Stream) -> Framed {
    Framed {
        stream,
        glyph: "│",
        text: String::new(),
    }
}

/// Draws the rail around a captured report whose first line is the verdict.
/// The rail closes on the footer, or on a bare `╰─` when the report has none. A report without a marked item stays flat: only the
/// footer marker is replaced by its plain indent.
pub(crate) fn frame(lines: Vec<Line>, color: bool) -> Vec<Line> {
    if !lines.iter().any(|(_, text)| text.starts_with(BRANCH)) {
        return lines
            .into_iter()
            .map(|(stream, text)| match text.strip_prefix(END) {
                Some(rest) => (stream, format!("  {rest}")),
                None => (stream, text),
            })
            .collect();
    }
    // A footer closes the rail even when a trailing hint follows it in the
    // flat layout.
    let mut lines = lines;
    if let Some(at) = lines.iter().rposition(|(_, text)| text.starts_with(END)) {
        let footer = lines.remove(at);
        lines.push(footer);
    }
    let total = lines.len();
    let mut framed: Vec<Framed> = Vec::with_capacity(total * 2);
    let mut closed = false;
    for (index, (stream, text)) in lines.into_iter().enumerate() {
        let separate = |framed: &mut Vec<Framed>| {
            let [.., before, prev] = framed.as_slice() else {
                framed.push(blank(stream));
                return;
            };
            // A blank rail line already separates the blocks, and a
            // one-line paragraph (a section heading) stays with what it
            // introduces.
            if prev.text.is_empty() || (prev.glyph == "│  " && before.text.is_empty()) {
                return;
            }
            framed.push(blank(stream));
        };
        let footer = text.strip_prefix(END);
        if index == 0 {
            framed.push(Framed {
                stream,
                glyph: "╭─ ",
                text,
            });
        } else if let Some(rest) = text.strip_prefix(BRANCH) {
            separate(&mut framed);
            framed.push(Framed {
                stream,
                glyph: "├─ ",
                text: rest.to_string(),
            });
        } else if let Some(rest) = footer {
            separate(&mut framed);
            closed = true;
            framed.push(Framed {
                stream,
                glyph: "╰─ ",
                text: rest.to_string(),
            });
        } else if text.is_empty() {
            separate(&mut framed);
        } else {
            let rest = text
                .strip_prefix(CHILD)
                .or_else(|| text.strip_prefix(END))
                .or_else(|| text.strip_prefix("  "))
                .unwrap_or(&text);
            framed.push(Framed {
                stream,
                glyph: "│  ",
                text: rest.to_string(),
            });
        }
    }
    if !closed {
        let stream = framed.last().map_or(Stream::Out, |last| last.stream);
        framed.push(Framed {
            stream,
            glyph: "╰─",
            text: String::new(),
        });
    }
    framed
        .into_iter()
        .map(
            |Framed {
                 stream,
                 glyph,
                 text,
             }| {
                let glyph = if color {
                    format!("\x1b[2m{glyph}\x1b[0m")
                } else {
                    glyph.to_string()
                };
                (stream, format!("{glyph}{text}"))
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(items: &[&str]) -> Vec<Line> {
        items
            .iter()
            .map(|text| (Stream::Err, (*text).to_string()))
            .collect()
    }

    fn texts(framed: Vec<Line>) -> Vec<String> {
        framed.into_iter().map(|(_, text)| text).collect()
    }

    #[test]
    fn a_report_with_items_opens_continues_branches_and_closes_on_the_footer() {
        let framed = frame(
            lines(&[
                "ERROR check",
                "  1 error(s)",
                &format!("{BRANCH}error[a]: first"),
                &format!("{CHILD}--> f:1:1"),
                &format!("{BRANCH}error[b]: second"),
                &format!("{END}1 file(s) affected"),
            ]),
            false,
        );
        assert_eq!(
            texts(framed),
            [
                "╭─ ERROR check",
                "│  1 error(s)",
                "│",
                "├─ error[a]: first",
                "│  --> f:1:1",
                "│",
                "├─ error[b]: second",
                "│",
                "╰─ 1 file(s) affected",
            ]
        );
    }

    #[test]
    fn a_footer_closes_the_rail_even_when_a_hint_follows_it() {
        let framed = frame(
            lines(&[
                "WARN lint",
                &format!("{BRANCH}one"),
                &format!("{END}1 file(s) affected"),
                "  hint",
            ]),
            false,
        );
        assert_eq!(
            texts(framed),
            [
                "╭─ WARN lint",
                "│",
                "├─ one",
                "│  hint",
                "│",
                "╰─ 1 file(s) affected",
            ]
        );
    }

    #[test]
    fn without_a_footer_the_rail_ends_on_a_bare_corner() {
        let framed = frame(
            lines(&["ERROR analyze", &format!("{BRANCH}one"), "  trailing"]),
            false,
        );
        assert_eq!(
            texts(framed),
            ["╭─ ERROR analyze", "│", "├─ one", "│  trailing", "╰─"]
        );
    }

    #[test]
    fn a_section_heading_stays_with_its_first_item() {
        let framed = frame(
            lines(&[
                "WARN lint",
                "",
                "Lint findings",
                &format!("{BRANCH}one"),
                &format!("{BRANCH}two"),
            ]),
            false,
        );
        assert_eq!(
            texts(framed),
            [
                "╭─ WARN lint",
                "│",
                "│  Lint findings",
                "├─ one",
                "│",
                "├─ two",
                "╰─",
            ]
        );
    }

    #[test]
    fn a_report_without_items_stays_flat() {
        let framed = frame(
            lines(&["PASS check", "  0 diagnostic(s)", &format!("{END}12 ms")]),
            true,
        );
        assert_eq!(
            texts(framed),
            ["PASS check", "  0 diagnostic(s)", "  12 ms"]
        );
    }

    #[test]
    fn glyphs_dim_with_color_and_keep_their_stream() {
        let framed = frame(
            vec![
                (Stream::Out, "ERROR check".to_string()),
                (Stream::Err, format!("{BRANCH}error[a]: first")),
            ],
            true,
        );
        assert_eq!(
            framed[0],
            (Stream::Out, "\x1b[2m╭─ \x1b[0mERROR check".into())
        );
        assert!(
            framed
                .iter()
                .skip(1)
                .all(|(stream, _)| *stream == Stream::Err)
        );
        assert_eq!(framed.last().unwrap().1, "\x1b[2m╰─\x1b[0m");
    }

    #[test]
    fn capture_collects_lines_instead_of_printing() {
        let captured = capture(|| {
            println!("a\nb");
            eprintln!();
        });
        assert_eq!(
            captured,
            vec![
                (Stream::Out, "a".to_string()),
                (Stream::Out, "b".to_string()),
                (Stream::Err, String::new()),
            ]
        );
    }
}
