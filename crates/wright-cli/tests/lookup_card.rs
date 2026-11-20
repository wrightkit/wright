use std::process::Command;

use serde_json::Value;
use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::lookup::LookupMatch;

fn lookup(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_wright"))
        .arg("lookup")
        .args(args)
        .output()
        .expect("lookup runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn workshop_card_and_json_preserve_catalog_descriptions() {
    let query = "Start Damage Modification";
    let catalog = Catalog::builtin().unwrap();
    let found = catalog.lookup(&Locale::new("en-US"), query).unwrap();
    let LookupMatch::Builtin {
        description: Some(description),
        signature: Some(signature),
        ..
    } = &found[0]
    else {
        panic!("catalog did not return the action and its description");
    };
    let text = lookup(&[query]);
    assert_eq!(
        text.lines().next(),
        Some("action Start Damage Modification")
    );
    assert!(text.contains(&description.text));
    let json: Value = serde_json::from_str(&lookup(&[query, "-f", "json"])).unwrap();
    let entry = &json["entries"][0];
    assert_eq!(entry["identity"], "startDamageModification");
    assert_eq!(entry["description"], description.text);
    assert_eq!(
        entry["signature"],
        "Start Damage Modification(receivers: Player|Array, damagers: Player|Array, damagePercent: Number, reevaluation: DamageReeval(None|Receivers and Damagers|Receivers Damagers and Damage Percent))"
    );
    assert_eq!(signature.params.len(), 4);
    for (index, param) in signature.params.iter().enumerate() {
        let description = &param
            .description
            .as_ref()
            .expect("catalog parameter description")
            .text;
        assert_eq!(
            entry["callable"]["parameters"][index]["description"],
            *description
        );
        assert!(
            text.contains(description),
            "missing parameter description: {text}"
        );
        assert!(text.contains(&format!(
            "{}: {}",
            param.name,
            param.param_type.as_deref().unwrap()
        )));
    }
    for member in [
        "None",
        "Receivers and Damagers",
        "Receivers Damagers and Damage Percent",
    ] {
        let domain_line = text
            .lines()
            .find(|line| line.contains("DamageReeval:"))
            .unwrap();
        assert_eq!(
            domain_line
                .split_once(':')
                .unwrap()
                .1
                .split(" | ")
                .filter(|name| name.trim() == member)
                .count(),
            1,
            "{domain_line}"
        );
    }
    for forbidden in ["query=", "summary:", "entry:"] {
        assert!(!text.contains(forbidden), "{text}");
    }
    let terminal = lookup(&[query, "--renderer", "terminal", "--color", "never"]);
    assert_eq!(terminal, text);
}

#[test]
fn enum_card_includes_every_member_even_with_limit_one() {
    let default = lookup(&["DamageReeval"]);
    let single = lookup(&["DamageReeval", "--limit", "1"]);
    assert_eq!(
        default.lines().take(2).collect::<Vec<_>>(),
        single.lines().collect::<Vec<_>>()
    );
    for query in ["DamageReeval", "Hero"] {
        let text = lookup(&[query, "--only", "enum", "--limit", "1"]);
        let json: Value = serde_json::from_str(&lookup(&[
            query, "--only", "enum", "--limit", "1", "-f", "json",
        ]))
        .unwrap();
        let entry = &json["entries"][0];
        assert_eq!(entry["kind"], "enum");
        assert_eq!(text.lines().next().unwrap(), format!("enum {query}"));
        let members = entry["enum"]["members"].as_array().unwrap();
        let listed: Vec<&str> = text
            .lines()
            .nth(1)
            .unwrap()
            .split_once(": ")
            .unwrap()
            .1
            .split(" | ")
            .collect();
        assert_eq!(
            listed,
            members
                .iter()
                .map(|member| member.as_str().unwrap())
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn candidates_are_single_lines_and_only_same_name_different_kinds_get_identities() {
    let text = lookup(&["Wait"]);
    let lines: Vec<&str> = text.lines().collect();
    let enum_line = lines
        .iter()
        .find(|line| line.starts_with("enum Wait"))
        .unwrap();
    let action_line = lines
        .iter()
        .find(|line| line.starts_with("action Wait Until"))
        .unwrap();
    assert_eq!(*enum_line, "enum Wait [Wait]");
    assert!(!action_line.contains('['));
    assert!(enum_line.len() <= "entry: enum name=Wait".len());
    assert!(action_line.len() <= "entry: action name=\"Wait Until\" signature=\"Wait Until(continueCondition: Boolean|Number, timeout: Number)\" identity=waitUntil".len());
    assert_eq!(lines.last(), Some(action_line));
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.starts_with("enum Wait") || line.starts_with("action Wait Until"))
            .count(),
        2
    );

    let health = lookup(&["Health"]);
    assert!(
        health
            .lines()
            .any(|line| line == "value Health [getHealth]"),
        "{health}"
    );
    assert!(
        health.lines().any(|line| line == "enum Health [Health]"),
        "{health}"
    );
    assert!(
        health
            .lines()
            .any(|line| line == "enumMember Health [Health.NORMAL]"),
        "{health}"
    );

    let text = lookup(&["Health", "--limit", "10"]);
    let json: Value =
        serde_json::from_str(&lookup(&["Health", "--limit", "10", "-f", "json"])).unwrap();
    let entries = json["entries"].as_array().unwrap();
    let candidates: Vec<&str> = text
        .lines()
        .skip_while(|line| !line.starts_with("enum Health ["))
        .collect();
    assert_eq!(candidates.len(), entries.len() - 1, "{text}");
    for (line, entry) in candidates.iter().zip(&entries[1..]) {
        let ambiguous = entries
            .iter()
            .any(|other| other["spelling"] == entry["spelling"] && other["kind"] != entry["kind"]);
        let head = entry["signature"]
            .as_str()
            .or_else(|| entry["spelling"].as_str())
            .unwrap();
        let identity = if ambiguous {
            format!(" [{}]", entry["identity"].as_str().unwrap())
        } else {
            String::new()
        };
        assert_eq!(
            *line,
            format!("{} {head}{identity}", entry["kind"].as_str().unwrap())
        );
    }
}

#[test]
fn requested_locale_selects_catalog_descriptions() {
    let query = "Start Damage Modification";
    let text = lookup(&[query, "--locale", "zh-CN"]);
    let json: Value =
        serde_json::from_str(&lookup(&[query, "--locale", "zh-CN", "-f", "json"])).unwrap();
    let catalog = Catalog::builtin().unwrap();
    let found = catalog.lookup(&Locale::new("zh-CN"), query).unwrap();
    let LookupMatch::Builtin {
        description: Some(description),
        signature: Some(signature),
        ..
    } = &found[0]
    else {
        panic!("catalog did not return the localized action");
    };
    assert!(!description.fallback);
    assert_eq!(json["entries"][0]["description"], description.text);
    assert!(text.contains(&description.text));
    for (index, param) in signature.params.iter().enumerate() {
        let description = param.description.as_ref().unwrap();
        assert!(!description.fallback);
        assert_eq!(
            json["entries"][0]["callable"]["parameters"][index]["description"],
            description.text
        );
        assert!(text.contains(&description.text));
    }
}

#[cfg(unix)]
#[test]
fn overpy_uses_the_same_card_and_candidate_layout() {
    use std::os::unix::fs::PermissionsExt;

    let dir = std::env::temp_dir().join(format!("wright-lookup-card-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let provider = dir.join("provider");
    std::fs::write(&provider, r##"#!/usr/bin/env python3
import json, sys
for line in sys.stdin:
    request = json.loads(line)
    method = request["method"]
    if method == "lpp/initialize":
        result = {"protocolVersion": "1.5", "serverInfo": {"name": "fake", "version": "0"},
                  "languages": [{"id": "opy", "extensions": ["opy"]}],
                  "capabilities": {"check": True, "compile": True, "reconstruct": False,
                                   "symbols": False, "definition": False, "references": False,
                                   "rename": False, "editValidation": False,
                                   "projectLoading": False, "lookup": True}}
    elif method == "lpp/lookup":
        result = {"entries": [
            {"identity": "opy:action/example", "kind": "action", "spelling": "example", "displayName": "example",
             "callable": {"parameters": [
                 {"name": "mode", "type": "Mode", "required": False, "default": "Mode.A",
                  "enum": {"domain": "opy:enum/Mode", "members": ["A", "B"]}}]}},
            {"identity": "opy:value/example", "kind": "value", "spelling": "example", "displayName": "example"}]}
    else:
        result = {}
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
"##).unwrap();
    std::fs::set_permissions(&provider, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = provider.to_str().unwrap();
    let args = ["--language", "opy", "--opy-provider", path, "example"];
    let text = lookup(&args);
    assert_eq!(
        text,
        "action example [opy:action/example]\n  example(mode: Mode=Mode.A)\n  Mode: A | B\nvalue example [opy:value/example]\n"
    );
    let terminal = lookup(&[
        "--language",
        "opy",
        "--opy-provider",
        path,
        "example",
        "--renderer",
        "terminal",
        "--color",
        "never",
    ]);
    assert_eq!(terminal, text);
    let _ = std::fs::remove_dir_all(dir);
}
