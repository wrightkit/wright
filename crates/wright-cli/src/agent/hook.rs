use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::ValueEnum;
use serde_json::{Value, json};
use wright_driver::result::exit;

use super::AgentError;
use super::config_file::{ensure_not_linked, read_config, write_config};
use super::stored::{Stored, load_failure, load_rejection, session_config};
use crate::cli::HookTarget;
use crate::present::RenderContext;

const COMMAND: &str = "wright agent hook post-edit ";
const CLAUDE_PROJECT_DIR: &str = "${CLAUDE_PROJECT_DIR:-.}";
const TIMEOUT_SECONDS: u64 = 60;

struct Spec {
    file: &'static str,
    variable: Option<&'static str>,
    input: String,
    group_matcher: Option<&'static str>,
}

fn spec(target: HookTarget, root: &Path) -> Result<Spec, AgentError> {
    Ok(match target {
        HookTarget::Claude => Spec {
            file: ".claude/settings.local.json",
            variable: Some(CLAUDE_PROJECT_DIR),
            input: CLAUDE_PROJECT_DIR.into(),
            group_matcher: Some("Edit|Write"),
        },
        HookTarget::Codex => Spec {
            file: ".codex/hooks.json",
            variable: Some(GIT_TOPLEVEL),
            input: git_anchor(root)?,
            group_matcher: Some("apply_patch"),
        },
        HookTarget::Cursor | HookTarget::Vscode => Spec {
            file: match target {
                HookTarget::Cursor => ".cursor/hooks.json",
                _ => ".github/copilot/settings.local.json",
            },
            variable: None,
            input: ".".into(),
            group_matcher: None,
        },
    })
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum HookStatus {
    Created(PathBuf),
    Replaced(PathBuf),
    UpToDate(PathBuf),
    DryRun(PathBuf),
}

const GIT_TOPLEVEL: &str = "$(git rev-parse --show-toplevel)";

/// The Codex anchor: the git toplevel, then the project root's path below it.
fn git_anchor(root: &Path) -> Result<String, AgentError> {
    let refused = || {
        AgentError::Rejected(
            "Codex sets no project directory variable, so the hook needs a git repository to find the project; run `git init` or use a project inside one".into(),
        )
    };
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(root)
        .output()
        .map_err(|_| refused())?;
    if !output.status.success() {
        return Err(refused());
    }
    let root = std::fs::canonicalize(root).map_err(|_| refused())?;
    let top = String::from_utf8_lossy(&output.stdout);
    let top = std::fs::canonicalize(top.trim()).map_err(|_| refused())?;
    let below = root.strip_prefix(&top).map_err(|_| refused())?;
    Ok(match below.to_string_lossy() {
        below if below.is_empty() => GIT_TOPLEVEL.to_string(),
        below => format!("{GIT_TOPLEVEL}/{below}"),
    })
}

fn shell_word(spec: &Spec, token: &str) -> String {
    let powershell = cfg!(windows) && spec.variable.is_none();
    let safe = |c: char| c.is_ascii_alphanumeric() || "-_./=:,@%+".contains(c);
    if !token.is_empty()
        && token.chars().all(safe)
        && !(powershell && (token.starts_with('@') || token == "--%"))
    {
        return token.to_string();
    }
    if let Some(variable) = spec.variable
        && let Some(rest) = token.strip_prefix(&spec.input)
    {
        let below = spec.input.strip_prefix(variable).unwrap_or_default();
        let mut quoted = String::new();
        for c in below.chars().chain(rest.chars()) {
            if "\\\"$`".contains(c) {
                quoted.push('\\');
            }
            quoted.push(c);
        }
        return format!("\"{variable}{quoted}\"");
    }
    if powershell {
        let mut quoted = String::from("'");
        for c in token.chars() {
            if matches!(c, '\'' | '‘' | '’' | '‚' | '‛') {
                quoted.push(c);
            }
            quoted.push(c);
        }
        quoted.push('\'');
        return quoted;
    }
    format!("'{}'", token.replace('\'', r"'\''"))
}

fn hook_command(target: HookTarget, spec: &Spec, stored: &Stored) -> String {
    let mut command = COMMAND.to_string();
    if target != HookTarget::Claude {
        command.push_str("--harness ");
        command.push_str(target.to_possible_value().expect("not skipped").get_name());
        command.push(' ');
    }
    command.push_str("--");
    for token in &stored.tokens {
        command.push(' ');
        command.push_str(&shell_word(spec, token));
    }
    command
}

fn command_hook(target: HookTarget, command: &str) -> Value {
    match target {
        HookTarget::Claude | HookTarget::Codex => {
            json!({"type": "command", "command": command, "timeout": TIMEOUT_SECONDS})
        }
        HookTarget::Cursor => json!({
            "type": "command", "command": command, "matcher": "Write", "timeout": TIMEOUT_SECONDS,
        }),
        HookTarget::Vscode => json!({
            "type": "command", "command": command, "matcher": "create|edit|str_replace_editor|apply_patch",
            "cwd": ".", "timeoutSec": TIMEOUT_SECONDS,
        }),
    }
}

fn is_wright_hook(hook: &Value) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .is_some_and(|command| command.starts_with(COMMAND))
}

pub(crate) fn install(
    root: &Path,
    target: HookTarget,
    check: &[String],
    force: bool,
    dry_run: bool,
) -> Result<(HookStatus, String), AgentError> {
    let spec = spec(target, root)?;
    let stored = Stored::new(&spec.input, root, check);
    let config = stored.session_config()?;
    if !force && let Some(failure) = load_failure(config) {
        return Err(load_rejection(
            "the Wright edit hook",
            &format!(
                "--hook {}",
                target.to_possible_value().expect("not skipped").get_name()
            ),
            &failure,
            check,
        ));
    }
    let path = root.join(spec.file);
    ensure_not_linked(root, &path)?;
    let mut config = read_config(&path)?.unwrap_or_else(|| json!({}));
    let event = if spec.group_matcher.is_some() {
        "PostToolUse"
    } else {
        "postToolUse"
    };
    let wrong_shape = || {
        AgentError::Rejected(format!(
            "{} does not hold a JSON object with a `hooks.{event}` list; fix it by hand",
            path.display()
        ))
    };
    let settings = config.as_object_mut().ok_or_else(wrong_shape)?;
    let needs_version = target == HookTarget::Cursor && !settings.contains_key("version");
    if target == HookTarget::Cursor
        && settings
            .get("version")
            .is_some_and(|version| version.as_u64().is_none_or(|number| number == 0))
    {
        return Err(AgentError::Rejected(format!(
            "{} needs a positive integer hook config `version`; fix it by hand",
            path.display()
        )));
    }
    let entries = settings
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(wrong_shape)?
        .entry(event)
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(wrong_shape)?;
    let command = hook_command(target, &spec, &stored);
    let hook = command_hook(target, &command);
    let mut ours = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let (hooks, matcher_matches) = match spec.group_matcher {
            Some(matcher) => (
                entry["hooks"].as_array().map_or(&[][..], Vec::as_slice),
                entry["matcher"].as_str() == Some(matcher),
            ),
            None => (std::slice::from_ref(entry), true),
        };
        for existing in hooks.iter().filter(|existing| is_wright_hook(existing)) {
            ours.push((index, matcher_matches && *existing == hook));
        }
    }
    let current = matches!(ours.as_slice(), [(_, true)]);
    if current && !needs_version {
        return Ok((HookStatus::UpToDate(path), command));
    }
    let replaced = !ours.is_empty();
    if !current && replaced && !force {
        return Err(AgentError::Rejected(format!(
            "{} already has a Wright hook that differs from the one `wright agent install` generates; pass --force to replace it, or remove it first",
            path.display()
        )));
    }
    if dry_run {
        return Ok((HookStatus::DryRun(path), command));
    }
    if !current {
        let first = ours.first().map_or(entries.len(), |(index, _)| *index);
        entries.retain_mut(|entry| {
            if spec.group_matcher.is_none() {
                return !is_wright_hook(entry);
            }
            let Some(hooks) = entry.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };
            let was_empty = hooks.is_empty();
            hooks.retain(|existing| !is_wright_hook(existing));
            was_empty || !hooks.is_empty()
        });
        let entry = match spec.group_matcher {
            Some(matcher) => json!({"matcher": matcher, "hooks": [hook]}),
            None => hook,
        };
        entries.insert(first.min(entries.len()), entry);
    }
    if needs_version {
        settings.insert("version".into(), json!(1));
    }
    write_config(&path, &config)?;
    let status = if replaced {
        HookStatus::Replaced(path)
    } else {
        HookStatus::Created(path)
    };
    Ok((status, command))
}

/// `PostToolUse` cannot undo the edit. Wright's own failures exit 1 on
/// stderr: for Claude Code exit 2 is the only code whose stderr reaches the
/// model, so exit 1 keeps a failure of Wright from being taken for a finding.
fn failed(message: &str) -> u8 {
    eprintln!("wright: {message}");
    exit::SOURCE_ERROR
}

/// The files an edit touched, as far as the harness's payload says.
fn edited_files(harness: HookTarget, event: &Value) -> Vec<String> {
    match harness {
        HookTarget::Claude => event["tool_input"]["file_path"]
            .as_str()
            .map(String::from)
            .into_iter()
            .collect(),
        // Captured from `codex exec` 0.169.0: `tool_name` is `apply_patch`,
        // `tool_input.command` is the patch text, `cwd` is the session
        // directory. The patch headers name each file, absolute or relative
        // to `cwd`.
        HookTarget::Codex => {
            let cwd = Path::new(event["cwd"].as_str().unwrap_or("."));
            event["tool_input"]["command"]
                .as_str()
                .into_iter()
                .flat_map(str::lines)
                .filter_map(|line| line.strip_prefix("*** "))
                .filter_map(|line| {
                    ["Add File: ", "Update File: ", "Delete File: ", "Move to: "]
                        .iter()
                        .find_map(|header| line.strip_prefix(header))
                })
                .map(|file| cwd.join(file.trim()).to_string_lossy().into_owned())
                .collect()
        }
        HookTarget::Cursor | HookTarget::Vscode => Vec::new(),
    }
}

pub(crate) fn post_edit(harness: HookTarget, check: &[String]) -> u8 {
    let input = match std::io::read_to_string(std::io::stdin()) {
        Ok(input) => input,
        Err(error) => return failed(&format!("could not read the hook input: {error}")),
    };
    let Ok(event) = serde_json::from_str::<Value>(&input) else {
        return failed("the hook input is not JSON");
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
    // A broken lint policy is a Wright failure, not a finding report.
    if let Err(diagnostic) = session.validate_lint_policy() {
        return failed(&diagnostic.message);
    }
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
        let (mut out, err) = crate::plain::render_lines(&envelope, &ctx);
        out.extend(err);
        out.join("\n")
    };
    if matches!(
        envelope.exit,
        exit::USAGE | exit::UNSUPPORTED | exit::INTERNAL
    ) {
        return failed(&report());
    }
    if envelope.diagnostics.is_empty() {
        return exit::SUCCESS;
    }
    let files = edited_files(harness, &event);
    if !files.is_empty()
        && files
            .iter()
            .all(|file| session.program_contains(file) == Some(false))
    {
        return exit::SUCCESS;
    }
    let response = match harness {
        HookTarget::Claude => {
            eprintln!("{}", report());
            return exit::USAGE;
        }
        HookTarget::Codex => json!({"hookSpecificOutput": {
            "hookEventName": "PostToolUse",
            "additionalContext": report(),
        }}),
        HookTarget::Cursor => json!({"additional_context": report()}),
        HookTarget::Vscode => json!({"additionalContext": report()}),
    };
    println!("{response}");
    exit::SUCCESS
}
