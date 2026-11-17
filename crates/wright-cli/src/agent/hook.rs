use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};
use wright_driver::result::exit;

use super::AgentError;
use super::config_file::{ensure_not_linked, read_config, write_config};
use super::stored::{Stored, load_failure, session_config};
use crate::cli::HookTarget;
use crate::present::RenderContext;

const FILE: &str = ".claude/settings.local.json";
const INPUT: &str = "${CLAUDE_PROJECT_DIR:-.}";
const COMMAND: &str = "wright agent hook post-edit --";
const MATCHER: &str = "Edit|Write";
const TIMEOUT_SECONDS: u64 = 60;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum HookStatus {
    Created(PathBuf),
    Replaced(PathBuf),
    UpToDate(PathBuf),
    DryRun(PathBuf),
}

/// A shell word for the entry's command line. A token that starts with the
/// project directory variable keeps the variable expandable.
fn shell_word(token: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "-_./=:,@%+".contains(c);
    if !token.is_empty() && token.chars().all(safe) {
        return token.to_string();
    }
    if let Some(rest) = token.strip_prefix(INPUT) {
        let mut quoted = String::new();
        for c in rest.chars() {
            if "\\\"$`".contains(c) {
                quoted.push('\\');
            }
            quoted.push(c);
        }
        return format!("\"{INPUT}{quoted}\"");
    }
    format!("'{}'", token.replace('\'', r"'\''"))
}

fn generated_hook(stored: &Stored) -> Value {
    let command = std::iter::once(COMMAND.to_string())
        .chain(stored.tokens.iter().map(|token| shell_word(token)))
        .collect::<Vec<_>>()
        .join(" ");
    json!({"type": "command", "command": command, "timeout": TIMEOUT_SECONDS})
}

fn is_wright_hook(hook: &Value) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .is_some_and(|command| command.starts_with(COMMAND))
}

fn hooks_of(group: &mut Value) -> Option<&mut Vec<Value>> {
    group.get_mut("hooks")?.as_array_mut()
}

pub(crate) fn install(
    root: &Path,
    _target: HookTarget,
    check: &[String],
    force: bool,
    dry_run: bool,
) -> Result<HookStatus, AgentError> {
    let stored = Stored::new(INPUT, root, check);
    let config = stored.session_config()?;
    if !force && let Some(failure) = load_failure(config) {
        return Err(AgentError::Rejected(format!(
            "the Wright edit hook would not load the project ({failure}); pass its input and options after `--`, for example `wright agent install --hook claude -- src/main.opy --kind opy`, or pass --force to register it anyway"
        )));
    }
    let path = root.join(FILE);
    ensure_not_linked(root, &path)?;
    let mut settings = read_config(&path)?.unwrap_or_else(|| json!({}));
    let wrong_shape = || {
        AgentError::Rejected(format!(
            "{} does not hold a JSON object with a `hooks.PostToolUse` list; fix it by hand",
            path.display()
        ))
    };
    let groups = settings
        .as_object_mut()
        .ok_or_else(wrong_shape)?
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(wrong_shape)?
        .entry("PostToolUse")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(wrong_shape)?;
    let hook = generated_hook(&stored);
    let mut ours = Vec::new();
    for (group_index, group) in groups.iter_mut().enumerate() {
        let matcher_matches = group.get("matcher") == Some(&json!(MATCHER));
        for (hook_index, existing) in hooks_of(group).into_iter().flatten().enumerate() {
            if is_wright_hook(existing) {
                ours.push((
                    group_index,
                    hook_index,
                    matcher_matches && *existing == hook,
                ));
            }
        }
    }
    if let [(_, _, true)] = ours[..] {
        return Ok(HookStatus::UpToDate(path));
    }
    if !ours.is_empty() && !force {
        return Err(AgentError::Rejected(format!(
            "{} already has a Wright hook that differs from the one `wright agent install` generates; pass --force to replace it, or remove it first",
            path.display()
        )));
    }
    let replaced = !ours.is_empty();
    let first = ours.first().map_or(groups.len(), |(group, _, _)| *group);
    let emptied: Vec<bool> = groups
        .iter_mut()
        .map(|group| {
            hooks_of(group).is_some_and(|hooks| {
                let before = hooks.len();
                hooks.retain(|existing| !is_wright_hook(existing));
                before > 0 && hooks.is_empty()
            })
        })
        .collect();
    let mut emptied = emptied.into_iter();
    groups.retain(|_| !emptied.next().unwrap_or(false));
    groups.insert(
        first.min(groups.len()),
        json!({"matcher": MATCHER, "hooks": [hook]}),
    );
    if dry_run {
        return Ok(HookStatus::DryRun(path));
    }
    write_config(&path, &settings)?;
    Ok(if replaced {
        HookStatus::Replaced(path)
    } else {
        HookStatus::Created(path)
    })
}

/// `PostToolUse` cannot undo the edit, and exit 2 is the only code that shows
/// stderr to the model. Every failure of Wright itself therefore exits 1.
fn failed(message: &str) -> u8 {
    eprintln!("wright: {message}");
    exit::SOURCE_ERROR
}

/// Checks the program after an edit. Exit 2 with the program's errors on
/// stderr; exit 0 with no output when there are none or the file is not in
/// the program.
pub(crate) fn post_edit(check: &[String]) -> u8 {
    let input = match std::io::read_to_string(std::io::stdin()) {
        Ok(input) => input,
        Err(error) => return failed(&format!("could not read the hook input: {error}")),
    };
    let Ok(event) = serde_json::from_str::<Value>(&input) else {
        return failed("the hook input is not JSON");
    };
    let Some(file) = event["tool_input"]["file_path"].as_str() else {
        return exit::SUCCESS;
    };
    let mut config = match session_config(check) {
        Ok(config) => config,
        Err(error) => return failed(error.message()),
    };
    config.selection = wright_driver::FindingSelection {
        severity: Some(wright_driver::Severity::Error),
        ..Default::default()
    };
    let mut session = match wright_driver::CompilerSession::new(config) {
        Ok(session) => session,
        Err(diagnostic) => return failed(&diagnostic.message),
    };
    let envelope = session.check();
    let report = || {
        let presentation = crate::present::Presentation::from_output(
            crate::cli::OutputFormatArg::Text,
            crate::cli::RendererArg::Plain,
            crate::cli::ColorArg::Never,
        );
        let base = session.input_root();
        let ctx = RenderContext {
            presentation,
            elapsed: Duration::ZERO,
            source_base: base.as_deref(),
            subject: None,
        };
        let (out, err) = crate::plain::render_lines(&envelope, &ctx);
        for line in out.iter().chain(&err) {
            eprintln!("{line}");
        }
    };
    match envelope.exit {
        exit::USAGE | exit::UNSUPPORTED | exit::INTERNAL => {
            report();
            exit::SOURCE_ERROR
        }
        _ if envelope.diagnostics.is_empty() => exit::SUCCESS,
        // When the program did not load, its membership is unknown and the
        // load failure is the likely effect of the edit.
        _ if session.program_contains(file) == Some(false) => exit::SUCCESS,
        _ => {
            report();
            exit::USAGE
        }
    }
}
