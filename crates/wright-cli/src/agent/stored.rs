use std::path::Path;

use super::AgentError;
use crate::serve::ServeArgs;

/// The stored check or server arguments, in the form `INPUT [options]` the
/// user gives after `--`. The input is optional and defaults to the project
/// directory; a relative input resolves against the harness's project
/// directory variable. `--mcp` and `--hook` share this form.
pub(super) struct Stored {
    /// The tokens as written into the harness entry, anchored to the
    /// harness's project directory variable.
    pub(super) tokens: Vec<String>,
    /// The same tokens anchored to the project root, for validation now.
    probe: Vec<String>,
}

impl Stored {
    pub(super) fn new(base: &str, root: &Path, given: &[String]) -> Stored {
        Stored {
            tokens: anchored_tokens(base, given),
            probe: anchored_tokens(&root.to_string_lossy(), given),
        }
    }

    pub(super) fn session_config(&self) -> Result<wright_driver::SessionConfig, AgentError> {
        session_config(&self.probe)
    }
}

fn anchor(base: &str, path: &str) -> String {
    match path.trim_start_matches("./") {
        _ if Path::new(path).is_absolute() => path.to_string(),
        "" | "." => base.to_string(),
        relative => format!("{base}/{relative}"),
    }
}

/// Anchors the input and the path-valued options (`--lint-config`,
/// `--rule-file`) to `base`, so the entry finds them from any working
/// directory.
pub(super) fn anchored_tokens(base: &str, given: &[String]) -> Vec<String> {
    const PATH_OPTIONS: [&str; 2] = ["--lint-config", "--rule-file"];
    let (input, options) = match given.split_first() {
        Some((first, rest)) if !first.starts_with('-') => (first.as_str(), rest),
        _ => (".", given),
    };
    let mut tokens = Vec::with_capacity(options.len() + 1);
    let mut path_next = false;
    for option in options {
        tokens.push(if std::mem::take(&mut path_next) {
            anchor(base, option)
        } else if let Some((name, value)) = option.split_once('=')
            && PATH_OPTIONS.contains(&name)
        {
            format!("{name}={}", anchor(base, value))
        } else {
            path_next = PATH_OPTIONS.contains(&option.as_str());
            option.clone()
        });
    }
    tokens.push(anchor(base, input));
    tokens
}

#[derive(clap::Parser)]
#[command(no_binary_name = true)]
struct StoredServe {
    #[command(flatten)]
    serve: ServeArgs,
}

/// The session configuration `serve` would start with for the tokens, or the
/// usage error that would stop it. `--transport` is fixed, so a stored token
/// cannot set it.
pub(super) fn session_config(
    tokens: &[String],
) -> Result<wright_driver::SessionConfig, AgentError> {
    let mut args = vec!["--transport".to_string(), "mcp".into()];
    args.extend_from_slice(tokens);
    let parsed = <StoredServe as clap::Parser>::try_parse_from(&args)
        .map_err(|error| AgentError::Usage(error.to_string()))?;
    parsed.serve.session_config().map_err(AgentError::Usage)
}

/// Loads the program the way `serve` does at startup. `None` means it loads.
pub(super) fn load_failure(config: wright_driver::SessionConfig) -> Option<String> {
    let failure = |diagnostic: wright_driver::Diagnostic| {
        format!("{}: {}", diagnostic.code, diagnostic.message)
    };
    wright_driver::CompilerSession::new(config)
        .map_err(failure)
        .and_then(|mut session| session.load().map(drop).map_err(failure))
        .err()
}

/// A POSIX shell and fish word that reads back as `token`: the token itself
/// when every character is plain, else single-quoted.
pub(super) fn shell_quote(token: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "-_./=:,@%+".contains(c);
    if !token.is_empty() && token.chars().all(safe) {
        return token.to_string();
    }
    format!("'{}'", token.replace('\'', r"'\''"))
}

/// The refusal for a registration that would not load the project. A locale
/// failure names the fix, because the default locale is not the project's.
pub(super) fn load_rejection(
    what: &str,
    flag: &str,
    failure: &str,
    given: &[String],
) -> AgentError {
    let example = if failure.starts_with("unknown-enum-member") {
        let mut args = Vec::new();
        let mut skip = false;
        for arg in given {
            if std::mem::take(&mut skip) || arg.starts_with("--locale=") {
                continue;
            }
            skip = arg == "--locale";
            if !skip {
                args.push(shell_quote(arg));
            }
        }
        args.extend(["--locale".into(), "zh-CN".into()]);
        format!(
            "the project may need another locale (a spelling is not in the current one): pass its input and options after `--`, for example `wright agent install {flag} -- {}`",
            args.join(" ")
        )
    } else {
        format!(
            "pass its input and options after `--`, for example `wright agent install {flag} -- src/main.opy --kind opy`"
        )
    };
    AgentError::Rejected(format!(
        "{what} would not load the project ({failure}); nothing was written; {example}, or pass --force to register it anyway"
    ))
}
