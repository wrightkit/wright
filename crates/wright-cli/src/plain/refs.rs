use super::*;

impl PlainReport for RefsResult {
    fn plain_report(
        &self,
        _envelope: &Envelope<Self>,
        _ctx: &RenderContext<'_>,
        out: &mut Vec<String>,
    ) {
        out.push(format!(
            "summary: {} {} reads={} writes={} calls={} rules={}",
            field("symbol", self.0["symbol"].as_str().unwrap_or("<unknown>")),
            field("kind", self.0["kind"].as_str().unwrap_or("symbol")),
            count(&self.0, "reads"),
            count(&self.0, "writes"),
            count(&self.0, "calls"),
            count(&self.0, "rules"),
        ));
        let references = self.0["references"]
            .as_array()
            .map_or(&[][..], Vec::as_slice);
        for reference in references {
            let mut items = vec![
                reference["kind"]
                    .as_str()
                    .unwrap_or("reference")
                    .to_string(),
            ];
            if let Some(at) = reference.get("span").and_then(at_field) {
                items.push(at);
            }
            if let Some(address) = rule_address(&reference["owningRule"]) {
                items.push(field("rule", address));
            } else if let Some(rule) = reference["rule"].as_u64() {
                items.push(format!("rule={rule}"));
            }
            if let Some(action) = reference["action"].as_u64() {
                items.push(format!("action={action}"));
            }
            if let Some(value) = reference.get("value").filter(|v| !v.is_null()) {
                items.push(field("value", &value.to_string()));
            }
            if let Some(excerpt) = reference["excerpt"].as_str() {
                items.push(field("excerpt", excerpt));
            }
            out.push(format!("ref: {}", items.join(" ")));
        }
        truncation_records(&self.0, "inspect refs", out);
        selected_from_payload(&self.0, "references", references.len(), out);
    }
}
