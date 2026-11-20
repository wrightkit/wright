//! Plain-text rendering of a `lookup` result: the first entry as a card,
//! every other entry on one line.

use crate::plain;

pub(crate) fn print(result: &serde_json::Value, language: &str) {
    if let Some(unavailable) = result.get("unavailable") {
        println!("UNSUPPORTED lookup {}", plain::field("language", language));
        eprintln!(
            "unavailable: {} {}",
            plain::field("owner", result["owner"].as_str().unwrap_or_default()),
            plain::field(
                "message",
                unavailable["message"].as_str().unwrap_or_default()
            )
        );
        return;
    }
    let entries = result["entries"].as_array().map_or(&[][..], Vec::as_slice);
    for (index, entry) in entries.iter().enumerate() {
        let spelling = entry["spelling"].as_str().unwrap_or_default();
        let kind = entry["kind"].as_str().unwrap_or("entry");
        let ambiguous = entries
            .iter()
            .any(|other| other["spelling"] == entry["spelling"] && other["kind"] != entry["kind"]);
        let identity = if ambiguous {
            format!(" [{}]", entry["identity"].as_str().unwrap_or_default())
        } else {
            String::new()
        };
        if index > 0 {
            println!(
                "{kind} {}{identity}",
                entry["signature"].as_str().unwrap_or(spelling)
            );
            continue;
        }

        println!("{kind} {spelling}{identity}");
        if let Some(description) = entry["description"].as_str() {
            println!("  {description}");
        }
        let params = entry["callable"]["parameters"]
            .as_array()
            .map_or(&[][..], Vec::as_slice);
        let mut domains = std::collections::HashSet::new();
        let mut enum_lines = Vec::new();
        for (domain, display) in entry
            .get("enum")
            .map(|domain| (domain, Some(spelling)))
            .into_iter()
            .chain(params.iter().filter_map(|param| {
                param
                    .get("enum")
                    .map(|domain| (domain, param["type"].as_str()))
            }))
            .chain(
                entry["parameter"]
                    .get("enum")
                    .map(|domain| (domain, entry["parameter"]["type"].as_str())),
            )
        {
            let identity = domain["domain"].as_str().unwrap_or_default();
            if !domains.insert(identity) {
                continue;
            }
            let name = if language == "opy" {
                display.unwrap_or("enum")
            } else {
                identity
            };
            let members: Vec<&str> = domain["members"]
                .as_array()
                .map_or(&[][..], Vec::as_slice)
                .iter()
                .filter_map(|member| member.as_str())
                .collect();
            enum_lines.push((name, members));
        }
        if let Some(signature) = entry["signature"].as_str() {
            let mut signature = signature.to_string();
            for (_, members) in &enum_lines {
                if !members.is_empty() {
                    signature = signature
                        .replace(&format!("({})", members.join("|")), "")
                        .replace(&format!("({} members)", members.len()), "");
                }
            }
            for param in params {
                let name = param["name"].as_str().unwrap_or_default();
                let param_type = param["type"].as_str().unwrap_or("any");
                for prefix in ["(", ", "] {
                    signature = signature.replace(
                        &format!("{prefix}{name}="),
                        &format!("{prefix}{name}: {param_type}="),
                    );
                    signature = signature.replace(
                        &format!("{prefix}{name}?"),
                        &format!("{prefix}{name}?: {param_type}"),
                    );
                }
            }
            println!("  {signature}");
        }
        for param in params {
            if let Some(description) = param["description"].as_str() {
                println!(
                    "  {}: {description}",
                    param["name"].as_str().unwrap_or_default()
                );
            }
        }
        if let Some(parameter) = entry.get("parameter") {
            if let Some(param_type) = parameter["type"].as_str() {
                println!("  type: {param_type}");
            }
            if let Some(description) = parameter["description"].as_str() {
                println!("  {description}");
            }
        }
        for (name, members) in enum_lines {
            println!("  {name}: {}", members.join(" | "));
        }
    }
}
