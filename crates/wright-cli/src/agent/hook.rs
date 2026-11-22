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

/// What differs between harnesses at install time. Both register a
/// `PostToolUse` command group in a JSON file.
struct Spec {
    file: &'static str,
    /// The shell expression for the harness's project directory.
    variable: &'static str,
    /// The text stored tokens are anchored to: the expression, then any
    /// literal path below it.
    input: String,
    matcher: &'static str,
    command_suffix: &'static str,
}

fn spec(target: HookTarget, root: &Path) -> Result<Spec, AgentError> {
    Ok(match target {
        // https://code.claude.com/docs/en/hooks
        HookTarget::Claude => Spec {
            file: ".claude/settings.local.json",
            variable: CLAUDE_PROJECT_DIR,
            input: CLAUDE_PROJECT_DIR.into(),
            matcher: "Edit|Write",
            command_suffix: "--",
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
            variable: GIT_TOPLEVEL,
            input: git_anchor(root)?,
            matcher: "apply_patch",
            command_suffix: "--harness codex --",
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
fn shell_word(variable: &str, input: &str, token: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "-_./=:,@%+".contains(c);
    if !token.is_empty() && token.chars().all(safe) {
        return token.to_string();
    }
    if let Some(rest) = token.strip_prefix(input) {
        let below = input.strip_prefix(variable).unwrap_or_default();
        let mut quoted = String::new();
        for c in below.chars().chain(rest.chars()) {
            if "\\\"$`".contains(c) {
                quoted.push('\\');
            }
            quoted.push(c);
        }
        return format!("\"{variable}{quoted}\"");
    }
    format!("'{}'", token.replace('\'', r"'\''"))
}

fn generated_hook(spec: &Spec, stored: &Stored) -> Value {
    let mut command = format!("{COMMAND}{}", spec.command_suffix);
    for token in &stored.tokens {
        command.push(' ');
        command.push_str(&shell_word(spec.variable, &spec.input, token));
    }
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
    let hook = generated_hook(&spec, &stored);
    let command = hook["command"].as_str().unwrap_or_default().to_string();
    let mut ours = Vec::new();
    for (group_index, group) in groups.iter_mut().enumerate() {
        let matcher_matches = group.get("matcher").and_then(Value::as_str) == Some(spec.matcher);
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
        return Ok((HookStatus::UpToDate(path), command));
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
        json!({"matcher": spec.matcher, "hooks": [hook]}),
    );
    if dry_run {
        return Ok((HookStatus::DryRun(path), command));
    }
    write_config(&path, &settings)?;
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
    }
}

/// Checks the program after an edit and answers in the harness's feedback
/// form: Claude Code gets exit 2 with the program's errors on stderr; Codex
/// gets exit 0 with `additionalContext` on stdout. Nothing is said when there
/// are no errors or no edited file is in the program. When the payload names
/// no file, the membership filter does not apply.
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
    match harness {
        HookTarget::Claude => {
            eprintln!("{}", report());
            exit::USAGE
        }
        HookTarget::Codex => {
            // https://developers.openai.com/codex/hooks: PostToolUse
            // `hookSpecificOutput.additionalContext` is added as developer
            // context; the answer is verified by the live run in #1109.
            println!(
                "{}",
                json!({"hookSpecificOutput": {
                    "hookEventName": "PostToolUse",
                    "additionalContext": report(),
                }})
            );
            exit::SUCCESS
        }
    }
}
