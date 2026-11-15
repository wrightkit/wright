//! Terminal rendering of the run-cache or baseline comparison: one
//! `Element cost` section with the total movement and the changed rules.

use serde_json::Value;

use super::{heading, pad, plural};
use crate::present::{RenderContext, dim};

pub(super) fn render(ctx: &RenderContext<'_>, comparison: Option<&Value>) {
    let Some(comparison) = comparison.filter(|c| c.is_object()) else {
        return;
    };
    heading(ctx, "Element cost");
    for row in comparison_rows(ctx, comparison) {
        println!("{row}");
    }
}

fn comparison_rows(ctx: &RenderContext<'_>, comparison: &Value) -> Vec<String> {
    const SHOWN: usize = 5;
    let color = ctx.presentation.color;
    let indent = pad(ctx, 1);
    let delta = comparison["totalDelta"].as_i64().unwrap_or(0);
    let previous = comparison["previousTotal"].as_i64().unwrap_or(0);
    let reference = comparison["reference"].as_str().unwrap_or("previous");
    let mut rows = vec![format!(
        "{indent}{delta:+} vs {reference} run · {} total {}",
        previous + delta,
        dim(&format!("(was {previous})"), color)
    )];
    let mut changes = Vec::new();
    for change in comparison["changed"].as_array().into_iter().flatten() {
        let d = change["delta"].as_i64().unwrap_or(0);
        changes.push((
            change["name"].as_str().unwrap_or("?").to_string(),
            format!("{} -> {} ({d:+})", change["before"], change["after"]),
        ));
    }
    for (key, state) in [("added", "added"), ("removed", "removed")] {
        for name in comparison[key].as_array().into_iter().flatten() {
            changes.push((name.as_str().unwrap_or("?").to_string(), state.to_string()));
        }
    }
    for (name, detail) in changes.iter().take(SHOWN) {
        rows.push(format!("{indent}{name}  {}", dim(detail, color)));
    }
    if changes.len() > SHOWN {
        rows.push(format!(
            "{indent}... {}",
            plural(
                changes.len() - SHOWN,
                "more changed rule",
                "more changed rules"
            )
        ));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{ColorArg, RendererArg};
    use crate::present::{OutputFormat, Presentation, RuntimeEnvironment};
    use serde_json::json;

    fn rows(columns: u16, value: &Value) -> Vec<String> {
        let environment = RuntimeEnvironment {
            github_actions: false,
            ci: false,
            stdout_terminal: true,
            no_color: false,
            force_color: false,
            term_dumb: false,
            columns: Some(columns),
        };
        let presentation = Presentation::resolve(
            OutputFormat::Text,
            RendererArg::Terminal,
            ColorArg::Never,
            environment,
        );
        let ctx = RenderContext {
            presentation,
            elapsed: std::time::Duration::ZERO,
            source_base: None,
            subject: None,
        };
        comparison_rows(&ctx, value)
    }

    #[test]
    fn comparison_reports_the_current_total_and_indents_changes() {
        let value = json!({
            "reference": "previous",
            "previousTotal": 28146,
            "totalDelta": 6,
            "changed": [{"name": "Load CN map", "before": 765, "after": 769, "delta": 4}],
            "added": ["New rule"],
            "removed": [],
        });
        assert_eq!(
            rows(100, &value),
            [
                "  +6 vs previous run · 28152 total (was 28146)",
                "  Load CN map  765 -> 769 (+4)",
                "  New rule  added",
            ]
        );
    }

    #[test]
    fn comparison_caps_changes_and_follows_the_compact_layout() {
        let changed: Vec<Value> = (0..7)
            .map(|i| json!({"name": format!("r{i}"), "before": 1, "after": 0, "delta": -1}))
            .collect();
        let value = json!({
            "reference": "baseline",
            "previousTotal": 10,
            "totalDelta": -7,
            "changed": changed,
        });
        let rows = rows(80, &value);
        assert_eq!(rows[0], "  -7 vs baseline run · 3 total (was 10)");
        assert_eq!(rows.len(), 7);
        assert_eq!(rows[6], "  ... 2 more changed rules");
    }
}
