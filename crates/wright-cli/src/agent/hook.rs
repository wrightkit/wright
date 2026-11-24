use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::ValueEnum;
use serde_json::{Value, json};
use wright_driver::result::exit;

use super::AgentError;
use super::config_file::{ensure_not_linked, read_config, write_config};
use super::stored::{Stored, load_failure, load_rejection, session_config, shell_quote};
use crate::cli::HookTarget;
use crate::present::RenderContext;

const COMMAND: &str = "wright agent hook post-edit ";
const CLAUDE_PROJECT_DIR: &str = "${CLAUDE_PROJECT_DIR:-.}";
const TIMEOUT_SECONDS: u64 = 60;

/// What differs between harnesses at install time. Claude Code and Codex
/// register a `PostToolUse` group that holds command hooks. Cursor and VS Code
/// register a flat `postToolUse` command hook.
struct Spec {
    file: &'static str,
    event: &'static str,
    /// The shell expression for the harness's project directory, when the
    /// harness sets one.
    variable: Option<&'static str>,
    /// The text stored tokens are anchored to: the expression, then any
    /// literal path below it.
    input: String,
    /// The matcher of the group the hook sits in; `None` for a flat hook.
    group_matcher: Option<&'static str>,
}

fn spec(target: HookTarget, root: &Path) -> Result<Spec, AgentError> {
    Ok(match target {
        // https://code.claude.com/docs/en/hooks
        HookTarget::Claude => Spec {
            file: ".claude/settings.local.json",
            event: "PostToolUse",
            variable: Some(CLAUDE_PROJECT_DIR),
            input: CLAUDE_PROJECT_DIR.into(),
            group_matcher: Some("Edit|Write"),
        },
        // https://developers.openai.com/codex/hooks (redirects to
        // learn.chatgpt.com/docs/hooks): project hooks live in
        // `.codex/hooks.json` and load only for a trusted project. Edits go
        // through `apply_patch`. A command runs in the session's working
        // directory and no project variable is set, so the input anchors to
        // the repository root, as the page's own example does, plus the
        // project's path below it.
        HookTarget::Codex => Spec {
            file: ".codex/hooks.json",
            event: "PostToolUse",
            variable: Some(GIT_TOPLEVEL),
            input: git_anchor(root)?,
            group_matcher: Some("apply_patch"),
        },
        // https://cursor.com/docs/agent/hooks (read 2026-11-23): project
        // hooks run from the project root, so the input anchors to `.`.
        HookTarget::Cursor => Spec {
            file: ".cursor/hooks.json",
            event: "postToolUse",
            variable: None,
            input: ".".into(),
            group_matcher: None,
        },
        // https://docs.github.com/en/copilot/reference/hooks-reference (read
        // 2026-11-23): the Copilot harness reads an inline `hooks` block from
        // `.github/copilot/settings.local.json`; the hook's `cwd` is the
        // repository root, so the input anchors to `.`.
        HookTarget::Vscode => Spec {
            file: ".github/copilot/settings.local.json",
            event: "postToolUse",
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

/// A shell word for the entry's command line. A token that starts with the
/// project directory keeps `variable` expandable; every other character of
/// the word stays literal.
fn shell_word(spec: &Spec, token: &str) -> String {
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
    shell_quote(token)
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
        // `Write` is the matcher value of the file-edit tool.
        HookTarget::Cursor => json!({
            "type": "command", "command": command, "matcher": "Write", "timeout": TIMEOUT_SECONDS,
        }),
        // The edit tools the reference lists, plus the `str_replace_editor`
        // and `apply_patch` names it maps to `Edit`. The matcher is a regex
        // tested against the whole `toolName`.
        HookTarget::Vscode => json!({
            "type": "command", "command": command,
            "matcher": "create|edit|str_replace_editor|apply_patch",
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
    let event = spec.event;
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
    let mut first = None;
    let mut duplicate = false;
    'entries: for (index, entry) in entries.iter().enumerate() {
        let (hooks, matcher_matches) = match spec.group_matcher {
            Some(matcher) => (
                entry["hooks"].as_array().map_or(&[][..], Vec::as_slice),
                entry["matcher"].as_str() == Some(matcher),
            ),
            None => (std::slice::from_ref(entry), true),
        };
        for existing in hooks.iter().filter(|existing| is_wright_hook(existing)) {
            if first.is_some() {
                duplicate = true;
                break 'entries;
            }
            first = Some((index, matcher_matches && *existing == hook));
        }
    }
    let current = !duplicate && matches!(first, Some((_, true)));
    if current && !needs_version {
        return Ok((HookStatus::UpToDate(path), command));
    }
    let replaced = first.is_some();
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
        let first = first.map_or(entries.len(), |(index, _)| index);
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
        entries.insert(first, entry);
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
        // Neither harness documents the edited path in its `postToolUse`
        // payload (Cursor's `afterFileEdit` does, but has no documented
        // output). The check runs without the membership filter.
        HookTarget::Cursor | HookTarget::Vscode => Vec::new(),
    }
}

/// Checks the program after an edit and answers in the harness's feedback
/// form: Claude Code gets exit 2 with the program's errors on stderr; the
/// others get exit 0 and the errors in the JSON field their reference names
/// (Codex `hookSpecificOutput.additionalContext`, Cursor `additional_context`,
/// VS Code `additionalContext`). Nothing is said when there are no errors or
/// no edited file is in the program. When the payload names no file, the
/// membership filter does not apply.
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
        // https://developers.openai.com/codex/hooks: added as developer
        // context; confirmed by the live run in #1109.
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
