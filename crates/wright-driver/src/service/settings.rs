//! The `project` result's read-only projection of the program's settings
//! block (#836): the simulation panel shows lobby settings verbatim and
//! defaults its run length from a `matchTimeLimit` member when the block
//! sets one.

use serde_json::{Value, json};
use workshop_rs::settings::{Settings, SettingsNode};

/// The settings block as a tree of `{name, value?, children?}` rows:
/// section names are the canonical keys (`main`, `lobby`, `workshop`),
/// member names the catalog or authored spelling, and leaf values the
/// emitted scalar text (`On`/`Off` for booleans).
pub(crate) fn settings_json(settings: &Settings) -> Value {
    Value::Array(settings.children.iter().map(node_json).collect())
}

fn node_json(node: &SettingsNode) -> Value {
    let (name, value, children) = match node {
        SettingsNode::Workshop { children, .. } => ("workshop", None, Some(children)),
        SettingsNode::Group { name, children, .. }
        | SettingsNode::RawGroup { name, children, .. } => (name.as_str(), None, Some(children)),
        SettingsNode::Number { name, value, .. } => (name.as_str(), Some(value.to_string()), None),
        SettingsNode::Bool { name, value, .. } => (
            name.as_str(),
            Some(if *value { "On" } else { "Off" }.to_string()),
            None,
        ),
        SettingsNode::Flag { name, .. } => (name.as_str(), Some("On".to_string()), None),
        SettingsNode::String { name, value, .. }
        | SettingsNode::Raw { name, value, .. }
        | SettingsNode::RawValue { name, value, .. } => (name.as_str(), Some(value.clone()), None),
        SettingsNode::List { name, elements, .. } => (
            name.as_str(),
            Some(
                elements
                    .iter()
                    .map(|element| match &element.variant {
                        Some(variant) => format!("{} {}", element.value, variant),
                        None => element.value.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            None,
        ),
    };
    let mut row = json!({ "name": name });
    if let Some(value) = value {
        row["value"] = Value::String(value);
    }
    if let Some(children) = children {
        row["children"] = Value::Array(children.iter().map(node_json).collect());
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_json_projects_groups_and_leaf_kinds() {
        let settings = Settings {
            span: None,
            children: vec![
                SettingsNode::Group {
                    name: "lobby".to_string(),
                    span: None,
                    children: vec![
                        SettingsNode::Number {
                            name: "matchTimeLimit".to_string(),
                            value: 600.0,
                            span: None,
                        },
                        SettingsNode::Bool {
                            name: "pauseGameOnDisconnect".to_string(),
                            value: true,
                            span: None,
                        },
                    ],
                },
                SettingsNode::Workshop {
                    span: None,
                    children: vec![SettingsNode::RawValue {
                        name: "scoreToWin".to_string(),
                        value: "2".to_string(),
                        span: None,
                    }],
                },
            ],
        };
        let value = settings_json(&settings);
        assert_eq!(
            value[0]["children"][0],
            json!({"name": "matchTimeLimit", "value": "600"})
        );
        assert_eq!(value[0]["children"][1]["value"], json!("On"));
        assert_eq!(value[1]["name"], json!("workshop"));
        assert_eq!(value[1]["children"][0]["value"], json!("2"));
    }

    #[test]
    fn list_elements_join_with_their_variant() {
        let node = SettingsNode::List {
            name: "enabledMaps".to_string(),
            span: None,
            elements: vec![workshop_rs::settings::SettingsListElement {
                value: "workshopChamber".to_string(),
                variant: Some("972777519512064577".to_string()),
                span: None,
            }],
        };
        assert_eq!(
            node_json(&node),
            json!({"name": "enabledMaps", "value": "workshopChamber 972777519512064577"})
        );
    }
}
