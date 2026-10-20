//! The `LintPackageResult` renderer impls (#844): `ResultPresentation`,
//! `PlainReport`, and `TerminalReport` for the `lint package` commands.
//! They live beside the package runner rather than in the shared renderer
//! modules, which are at their size budget; the records, tags, and layout
//! they produce follow ADR-0030 identically.

use wright_driver::result::{Envelope, LintPackageResult, LintPackageRow};

use crate::plain::{Line, PlainReport, Stream, expand_all, field, truncated_record};
use crate::present::terminal::{TerminalReport, heading, line, next, note, plural};
use crate::present::{RenderContext, ResultPresentation};

/// The status-count breakdown `summary:`/metadata/headline share: the row
/// count leads — `packages=N` — then each present status word reports its
/// own count in the record contract's order. A report whose rows all share
/// one status adds no breakdown (`5 installed` repeats `installed` five
/// times' worth of nothing); `search` appends the API's `total_count`.
fn status_counts(result: &LintPackageResult) -> Vec<(String, usize)> {
    const ORDER: &[&str] = &[
        "found",
        "installed",
        "unchanged",
        "updated",
        "removed",
        "pruned",
        "damaged",
        "unavailable",
        "error",
    ];
    let mut counts: Vec<(String, usize)> = Vec::new();
    for status in ORDER {
        let n = result
            .packages
            .iter()
            .filter(|row| row.status == *status)
            .count();
        if n > 0 {
            counts.push(((*status).to_string(), n));
        }
    }
    if counts.len() == 1 {
        counts.clear();
    }
    counts
}

/// The `lint package` metadata line: `N package(s)` plus the status
/// breakdown when the rows are mixed, and the search API's total.
fn metadata(result: &LintPackageResult) -> String {
    let mut metadata = format!("{} package(s)", result.packages.len());
    if let Some(total) = result.total.filter(|total| {
        *total as usize != result.packages.len() || result.incomplete.unwrap_or(false)
    }) {
        metadata.push_str(&format!(" of {total} on GitHub"));
    }
    let counts = status_counts(result);
    if !counts.is_empty() {
        metadata.push_str(&format!(
            " ({})",
            counts
                .iter()
                .map(|(status, n)| format!("{n} {status}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    metadata
}

impl ResultPresentation for LintPackageResult {
    fn metadata(&self) -> Option<String> {
        Some(metadata(self))
    }
    // Not an artifact stream: the plain renderer emits `package:` records
    // and the terminal renderer its own layout — `render_body` never runs.
    fn render_body(&self, _ctx: &RenderContext<'_>) {}
}

/// One `package:` record per row: `source` and `status` lead, then the
/// row's own facts in a fixed order — the installed `sha`, the remote head
/// `remote`, the entry `dir`, the rule ids `rules`, the `downloaded` flag,
/// and `code`/`message`/`description` when the row carries them.
fn package_record(row: &LintPackageRow) -> String {
    let mut items = vec![field("source", &row.source), field("status", &row.status)];
    if let Some(sha) = &row.sha {
        items.push(field("sha", sha));
    }
    if let Some(remote) = &row.remote {
        items.push(field("remote", remote));
    }
    if let Some(dir) = &row.dir {
        items.push(field("dir", dir));
    }
    if !row.rules.is_empty() {
        items.push(field("rules", &row.rules.join(",")));
    }
    if let Some(downloaded) = row.downloaded {
        items.push(format!("downloaded={downloaded}"));
    }
    if let Some(code) = &row.code {
        items.push(field("code", code));
    }
    if let Some(message) = &row.message {
        items.push(field("message", message));
    }
    if let Some(description) = &row.description {
        items.push(field("description", description));
    }
    format!("package: {}", items.join(" "))
}

/// `summary:` counts the rows and names the store root — status counts
/// appear only when the rows are mixed — then one `package:` record per
/// row, capped at the default page unless `--all` lifted it.
impl PlainReport for LintPackageResult {
    fn plain_report(
        &self,
        envelope: &Envelope<Self>,
        ctx: &RenderContext<'_>,
        out: &mut Vec<Line>,
    ) {
        let mut fields = vec![format!("packages={}", self.packages.len())];
        if let Some(total) = self.total.filter(|t| *t as usize != self.packages.len()) {
            fields.push(format!("total={total}"));
        }
        fields.extend(
            status_counts(self)
                .into_iter()
                .map(|(status, n)| format!("{status}={n}")),
        );
        fields.push(field("store", &self.store));
        out.push((Stream::Out, format!("summary: {}", fields.join(" "))));
        let limit = if ctx.presentation.all { usize::MAX } else { 20 };
        for row in self.packages.iter().take(limit) {
            out.push((Stream::Out, package_record(row)));
        }
        if self.packages.len() > limit {
            out.push((
                Stream::Out,
                truncated_record(
                    &[("packages", limit, self.packages.len())],
                    &expand_all(envelope),
                ),
            ));
        }
    }
}

/// One item per row — `owner/repo@<sha7> — status` — with the row's
/// message, rules, and entry directory as secondary notes, capped at
/// twenty unless `--all` lifted it.
impl TerminalReport for LintPackageResult {
    fn headline(&self) -> Option<String> {
        let mut parts = vec![plural(self.packages.len(), "package", "packages")];
        if let Some(total) = self.total.filter(|t| *t as usize != self.packages.len()) {
            parts.push(format!("{total} on GitHub"));
        }
        Some(
            parts
                .into_iter()
                .chain(
                    status_counts(self)
                        .into_iter()
                        .map(|(status, n)| format!("{n} {status}")),
                )
                .collect::<Vec<_>>()
                .join(" · "),
        )
    }

    fn body(&self, ctx: &RenderContext<'_>) {
        heading(ctx, "Lint packages");
        if self.packages.is_empty() {
            line(ctx, 1, "none");
        }
        let limit = if ctx.presentation.all { usize::MAX } else { 20 };
        for row in self.packages.iter().take(limit) {
            let mut head = row.source.clone();
            if let Some(sha) = &row.sha {
                head.push('@');
                head.push_str(&sha[..sha.len().min(7)]);
            }
            head.push_str(" — ");
            head.push_str(&row.status);
            if row.downloaded == Some(false) {
                head.push_str(" (reused)");
            }
            if let Some(remote) = &row.remote
                && row.status == "updated"
            {
                head.push_str(&format!(" → {}", &remote[..remote.len().min(7)]));
            }
            line(ctx, 1, &head);
            if let Some(description) = &row.description {
                note(ctx, 2, description);
            }
            if let Some(message) = &row.message {
                note(ctx, 2, message);
            }
            if !row.rules.is_empty() {
                note(ctx, 2, &format!("rules: {}", row.rules.join(", ")));
            }
            if let Some(dir) = &row.dir {
                note(ctx, 2, dir);
            }
        }
        if self.packages.len() > limit {
            line(
                ctx,
                1,
                &format!(
                    "... {} more (--all to see every package)",
                    self.packages.len() - limit
                ),
            );
        }
        let value = serde_json::to_value(self).unwrap_or_default();
        next(ctx, &value);
    }
}
