use wright_driver::result::{Envelope, LintPackageResult, LintPackageRow};

use crate::plain::{PlainReport, expand_all, field, truncated_record};
use crate::present::terminal::{TerminalReport, heading, line, next, note, plural};
use crate::present::{RenderContext, ResultPresentation};

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
}

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

impl PlainReport for LintPackageResult {
    fn plain_report(
        &self,
        envelope: &Envelope<Self>,
        ctx: &RenderContext<'_>,
        out: &mut Vec<String>,
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
        out.push(format!("summary: {}", fields.join(" ")));
        let limit = if ctx.presentation.all { usize::MAX } else { 20 };
        for row in self.packages.iter().take(limit) {
            out.push(package_record(row));
        }
        if self.packages.len() > limit {
            out.push(truncated_record(
                &[("packages", limit, self.packages.len())],
                &expand_all(envelope),
            ));
        }
    }
}

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
