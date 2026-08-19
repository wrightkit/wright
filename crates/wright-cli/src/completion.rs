//! Shell completion generation, detection, installation, and refresh (#186).
//!
//! Provides the authoritative lifecycle for shell completions generated from
//! the `clap` command model. Supports pure generation (`wright completion <shell>`),
//! automatic or explicit installation into conventional user-local locations
//! (`wright completion install [shell]`), and idempotent refresh during `wright update`.

use std::path::{Path, PathBuf};

use clap::CommandFactory;

use crate::cli::{Cli, CompletionInstallArgs, ShellArg};
use crate::CLI_NAME;

mod exit {
    pub(super) const SUCCESS: u8 = 0;
    pub(super) const USER_ERROR: u8 = 1;
    pub(super) const USAGE: u8 = 2;
    pub(super) const INTERNAL: u8 = 4;
}

/// A failure of completion operations.
#[derive(Debug)]
pub(crate) enum CompletionError {
    Usage(String),
    UndetectedShell(String),
    Failed(String),
}

impl CompletionError {
    pub(crate) fn usage(message: impl Into<String>) -> Self {
        CompletionError::Usage(message.into())
    }
    pub(crate) fn undetected(message: impl Into<String>) -> Self {
        CompletionError::UndetectedShell(message.into())
    }
    pub(crate) fn failed(message: impl Into<String>) -> Self {
        CompletionError::Failed(message.into())
    }

    pub(crate) fn exit_code(&self) -> u8 {
        match self {
            CompletionError::Usage(_) => exit::USAGE,
            CompletionError::UndetectedShell(_) => exit::USER_ERROR,
            CompletionError::Failed(_) => exit::INTERNAL,
        }
    }

    pub(crate) fn message(&self) -> &str {
        match self {
            CompletionError::Usage(msg)
            | CompletionError::UndetectedShell(msg)
            | CompletionError::Failed(msg) => msg,
        }
    }
}

/// The result status of an installation operation.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum InstallStatus {
    Created(PathBuf),
    Updated(PathBuf),
    UpToDate(PathBuf),
    DryRun(PathBuf),
}

impl InstallStatus {
    #[allow(dead_code)]
    pub(crate) fn path(&self) -> &Path {
        match self {
            InstallStatus::Created(p)
            | InstallStatus::Updated(p)
            | InstallStatus::UpToDate(p)
            | InstallStatus::DryRun(p) => p,
        }
    }
}

/// Generate the completion script bytes for a given shell from the authoritative `clap` command model.
pub(crate) fn generate_script(shell: ShellArg) -> Vec<u8> {
    let mut command = Cli::command();
    let mut buffer = Vec::new();
    clap_complete::generate(shell.to_clap_shell(), &mut command, CLI_NAME, &mut buffer);
    buffer
}

/// The standard filename for the completion script of a given shell.
pub(crate) fn filename_for(shell: ShellArg) -> &'static str {
    match shell {
        ShellArg::Bash => "wright",
        ShellArg::Zsh => "_wright",
        ShellArg::Fish => "wright.fish",
        ShellArg::PowerShell => "_wright.ps1",
    }
}

fn env_var_non_empty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|val| !val.trim().is_empty())
}

/// Detect the active shell from environment variables.
pub(crate) fn detect_shell() -> Result<ShellArg, CompletionError> {
    if let Some(override_shell) = env_var_non_empty("WRIGHT_SHELL") {
        return parse_shell_name(&override_shell).ok_or_else(|| {
            CompletionError::usage(format!(
                "invalid shell '{override_shell}' in WRIGHT_SHELL (expected bash|zsh|fish|powershell)"
            ))
        });
    }

    // Check $SHELL (POSIX path to shell executable)
    if let Some(shell_path) = env_var_non_empty("SHELL") {
        let shell_name = Path::new(&shell_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&shell_path);
        if let Some(shell) = parse_shell_name(shell_name) {
            return Ok(shell);
        }
    }

    // Check shell-specific environment variables
    if env_var_non_empty("ZSH_VERSION").is_some() || env_var_non_empty("ZDOTDIR").is_some() {
        return Ok(ShellArg::Zsh);
    }
    if env_var_non_empty("BASH_VERSION").is_some() {
        return Ok(ShellArg::Bash);
    }
    if env_var_non_empty("FISH_VERSION").is_some() {
        return Ok(ShellArg::Fish);
    }
    if env_var_non_empty("PSModulePath").is_some()
        || env_var_non_empty("POWERSHELL_DISTRIBUTION_CHANNEL").is_some()
        || env_var_non_empty("PSExecutionPolicyPreference").is_some()
    {
        return Ok(ShellArg::PowerShell);
    }

    // Windows fallback if no other shell detected
    if cfg!(windows) {
        return Ok(ShellArg::PowerShell);
    }

    Err(CompletionError::undetected(
        "could not automatically detect your shell; specify it explicitly with `wright completion install <bash|zsh|fish|powershell>`",
    ))
}

fn parse_shell_name(name: &str) -> Option<ShellArg> {
    let lower = name.to_ascii_lowercase();
    let name = lower.trim();
    if name == "zsh" || name.starts_with("zsh") {
        Some(ShellArg::Zsh)
    } else if name == "bash" || name.starts_with("bash") {
        Some(ShellArg::Bash)
    } else if name == "fish" || name.starts_with("fish") {
        Some(ShellArg::Fish)
    } else if name == "pwsh"
        || name == "powershell"
        || name.starts_with("pwsh")
        || name.starts_with("powershell")
    {
        Some(ShellArg::PowerShell)
    } else {
        None
    }
}

fn user_home_dir() -> Result<PathBuf, CompletionError> {
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            return Ok(PathBuf::from(home));
        }
    }
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        if !userprofile.is_empty() {
            return Ok(PathBuf::from(userprofile));
        }
    }
    Err(CompletionError::failed(
        "could not determine user home directory (HOME or USERPROFILE environment variable not set)",
    ))
}

fn xdg_data_home() -> Result<PathBuf, CompletionError> {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.is_empty() {
            return Ok(PathBuf::from(xdg));
        }
    }
    let home = user_home_dir()?;
    Ok(home.join(".local").join("share"))
}

fn xdg_config_home() -> Result<PathBuf, CompletionError> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Ok(PathBuf::from(xdg));
        }
    }
    let home = user_home_dir()?;
    Ok(home.join(".config"))
}

/// Determine the default conventional installation directory for a given shell.
pub(crate) fn default_dir_for(shell: ShellArg) -> Result<PathBuf, CompletionError> {
    if let Ok(override_dir) = std::env::var("WRIGHT_COMPLETION_DIR") {
        if !override_dir.is_empty() {
            return Ok(PathBuf::from(override_dir));
        }
    }

    let home = user_home_dir()?;
    let data_home = xdg_data_home()?;
    let config_home = xdg_config_home()?;

    match shell {
        ShellArg::Fish => {
            let config_fish = config_home.join("fish").join("completions");
            let vendor_fish = data_home.join("fish").join("vendor_completions.d");
            if vendor_fish.is_dir() && !config_fish.is_dir() {
                Ok(vendor_fish)
            } else {
                Ok(config_fish)
            }
        }
        ShellArg::Bash => {
            let xdg_bash = data_home.join("bash-completion").join("completions");
            let legacy_bash = home.join(".bash_completion.d");
            if legacy_bash.is_dir() && !xdg_bash.is_dir() {
                Ok(legacy_bash)
            } else {
                Ok(xdg_bash)
            }
        }
        ShellArg::Zsh => {
            if let Ok(custom) = std::env::var("ZSH_CUSTOM") {
                if !custom.is_empty() {
                    let custom_path = PathBuf::from(custom);
                    let completions = custom_path.join("completions");
                    if completions.is_dir() || custom_path.is_dir() {
                        return Ok(completions);
                    }
                }
            }
            let oh_my_zsh_custom = home.join(".oh-my-zsh").join("custom").join("completions");
            if oh_my_zsh_custom.is_dir() {
                return Ok(oh_my_zsh_custom);
            }
            let oh_my_zsh = home.join(".oh-my-zsh");
            if oh_my_zsh.is_dir() {
                return Ok(oh_my_zsh_custom);
            }
            let zfunc = home.join(".zfunc");
            if zfunc.is_dir() {
                return Ok(zfunc);
            }
            let dot_zsh = home.join(".zsh").join("completions");
            if dot_zsh.is_dir() {
                return Ok(dot_zsh);
            }
            Ok(data_home.join("zsh").join("site-functions"))
        }
        ShellArg::PowerShell => {
            if cfg!(windows) {
                let ps_docs = home.join("Documents").join("PowerShell").join("Scripts");
                let win_ps_docs = home
                    .join("Documents")
                    .join("WindowsPowerShell")
                    .join("Scripts");
                if win_ps_docs.is_dir() && !ps_docs.is_dir() {
                    Ok(win_ps_docs)
                } else {
                    Ok(ps_docs)
                }
            } else {
                Ok(config_home.join("powershell").join("completions"))
            }
        }
    }
}

/// Return all candidate conventional directories for a given shell.
pub(crate) fn candidate_dirs_for(shell: ShellArg) -> Vec<PathBuf> {
    if let Ok(override_dir) = std::env::var("WRIGHT_COMPLETION_DIR") {
        if !override_dir.is_empty() {
            return vec![PathBuf::from(override_dir)];
        }
    }

    let mut dirs = Vec::new();
    let Ok(home) = user_home_dir() else {
        return dirs;
    };
    let Ok(data_home) = xdg_data_home() else {
        return dirs;
    };
    let Ok(config_home) = xdg_config_home() else {
        return dirs;
    };

    match shell {
        ShellArg::Fish => {
            dirs.push(config_home.join("fish").join("completions"));
            dirs.push(data_home.join("fish").join("vendor_completions.d"));
        }
        ShellArg::Bash => {
            dirs.push(data_home.join("bash-completion").join("completions"));
            dirs.push(home.join(".bash_completion.d"));
        }
        ShellArg::Zsh => {
            if let Ok(custom) = std::env::var("ZSH_CUSTOM") {
                if !custom.is_empty() {
                    dirs.push(PathBuf::from(custom).join("completions"));
                }
            }
            dirs.push(home.join(".oh-my-zsh").join("custom").join("completions"));
            dirs.push(home.join(".zfunc"));
            dirs.push(home.join(".zsh").join("completions"));
            dirs.push(data_home.join("zsh").join("site-functions"));
        }
        ShellArg::PowerShell => {
            dirs.push(home.join("Documents").join("PowerShell").join("Scripts"));
            dirs.push(
                home.join("Documents")
                    .join("WindowsPowerShell")
                    .join("Scripts"),
            );
            dirs.push(config_home.join("powershell").join("completions"));
            dirs.push(data_home.join("powershell").join("Completions"));
        }
    }

    dirs
}

/// Install the generated completion script for a shell into `explicit_dir` or the default location.
pub(crate) fn install_for_shell(
    shell: ShellArg,
    explicit_dir: Option<&Path>,
    dry_run: bool,
    force: bool,
) -> Result<InstallStatus, CompletionError> {
    let target_dir = match explicit_dir {
        Some(dir) => dir.to_path_buf(),
        None => default_dir_for(shell)?,
    };
    let target_file = target_dir.join(filename_for(shell));
    let content = generate_script(shell);

    if dry_run {
        return Ok(InstallStatus::DryRun(target_file));
    }

    if target_file.is_file() {
        let existing = std::fs::read(&target_file).map_err(|err| {
            CompletionError::failed(format!(
                "could not read existing completion file {}: {err}",
                target_file.display()
            ))
        })?;
        if existing == content && !force {
            return Ok(InstallStatus::UpToDate(target_file));
        }
        std::fs::write(&target_file, &content).map_err(|err| {
            CompletionError::failed(format!(
                "could not update completion file {}: {err}",
                target_file.display()
            ))
        })?;
        return Ok(InstallStatus::Updated(target_file));
    }

    if let Err(err) = std::fs::create_dir_all(&target_dir) {
        return Err(CompletionError::failed(format!(
            "could not create completion directory {}: {err}",
            target_dir.display()
        )));
    }

    std::fs::write(&target_file, &content).map_err(|err| {
        CompletionError::failed(format!(
            "could not write completion file {}: {err}",
            target_file.display()
        ))
    })?;

    Ok(InstallStatus::Created(target_file))
}

fn print_guidance(shell: ShellArg, target_file: &Path) {
    let target_dir = target_file.parent().unwrap_or(target_file);
    match shell {
        ShellArg::Zsh => {
            let path_str = target_dir.to_string_lossy();
            if path_str.contains(".oh-my-zsh") {
                println!("note: completion installed into Oh My Zsh custom completions directory");
            } else {
                println!(
                    "note: ensure {} is in your zsh $fpath (e.g. fpath=({} $fpath) in ~/.zshrc)",
                    target_dir.display(),
                    target_dir.display()
                );
            }
        }
        ShellArg::Bash => {
            println!(
                "note: bash completions in {} are loaded automatically when bash-completion is active",
                target_dir.display()
            );
        }
        ShellArg::Fish => {
            println!(
                "note: fish autoloads completions from {}",
                target_dir.display()
            );
        }
        ShellArg::PowerShell => {
            println!(
                "note: add '. \"{}\"' to your PowerShell $PROFILE if not already autoloaded",
                target_file.display()
            );
        }
    }
}

/// Run the `completion install` workflow.
pub(crate) fn run_install(args: &CompletionInstallArgs) -> Result<u8, CompletionError> {
    if args.all {
        let shells = [
            ShellArg::Bash,
            ShellArg::Zsh,
            ShellArg::Fish,
            ShellArg::PowerShell,
        ];
        for shell in shells {
            let status = install_for_shell(shell, args.dir.as_deref(), args.dry_run, args.force)?;
            match &status {
                InstallStatus::Created(path) => {
                    println!(
                        "==> installed {} completion to {}",
                        shell.as_str(),
                        path.display()
                    );
                }
                InstallStatus::Updated(path) => {
                    println!(
                        "==> updated {} completion in {}",
                        shell.as_str(),
                        path.display()
                    );
                }
                InstallStatus::UpToDate(path) => {
                    println!(
                        "{} completion in {} is already up to date",
                        shell.as_str(),
                        path.display()
                    );
                }
                InstallStatus::DryRun(path) => {
                    println!(
                        "would install {} completion to {}",
                        shell.as_str(),
                        path.display()
                    );
                }
            }
        }
        return Ok(exit::SUCCESS);
    }

    let shell = match args.effective_shell() {
        Some(s) => s,
        None => detect_shell()?,
    };

    let status = install_for_shell(shell, args.dir.as_deref(), args.dry_run, args.force)?;
    match &status {
        InstallStatus::Created(path) => {
            println!(
                "==> installed {} completion to {}",
                shell.as_str(),
                path.display()
            );
            print_guidance(shell, path);
        }
        InstallStatus::Updated(path) => {
            println!(
                "==> updated {} completion in {}",
                shell.as_str(),
                path.display()
            );
            print_guidance(shell, path);
        }
        InstallStatus::UpToDate(path) => {
            println!(
                "{} completion in {} is already up to date",
                shell.as_str(),
                path.display()
            );
        }
        InstallStatus::DryRun(path) => {
            println!(
                "would install {} completion to {}",
                shell.as_str(),
                path.display()
            );
            print_guidance(shell, path);
        }
    }

    Ok(exit::SUCCESS)
}

/// Refresh any existing completion files found across conventional candidate locations.
/// Returns the number of refreshed files.
pub(crate) fn refresh_installed_completions() -> Result<usize, String> {
    let shells = [
        ShellArg::Bash,
        ShellArg::Zsh,
        ShellArg::Fish,
        ShellArg::PowerShell,
    ];
    let mut refreshed = 0;
    for shell in shells {
        let candidate_dirs = candidate_dirs_for(shell);
        let filename = filename_for(shell);
        for dir in candidate_dirs {
            let target_file = dir.join(filename);
            if target_file.is_file() {
                match install_for_shell(shell, Some(&dir), false, false) {
                    Ok(InstallStatus::Updated(p)) => {
                        println!(
                            "==> refreshed {} completion in {}",
                            shell.as_str(),
                            p.display()
                        );
                        refreshed += 1;
                    }
                    Ok(InstallStatus::Created(p)) => {
                        println!(
                            "==> refreshed {} completion in {}",
                            shell.as_str(),
                            p.display()
                        );
                        refreshed += 1;
                    }
                    Ok(InstallStatus::UpToDate(_)) => {}
                    Ok(InstallStatus::DryRun(_)) => {}
                    Err(err) => {
                        eprintln!(
                            "warning: could not refresh {} completion in {}: {}",
                            shell.as_str(),
                            target_file.display(),
                            err.message()
                        );
                    }
                }
            }
        }
    }
    Ok(refreshed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_shell_name_handles_standard_names_and_aliases() {
        assert_eq!(parse_shell_name("bash"), Some(ShellArg::Bash));
        assert_eq!(parse_shell_name("zsh"), Some(ShellArg::Zsh));
        assert_eq!(parse_shell_name("fish"), Some(ShellArg::Fish));
        assert_eq!(parse_shell_name("powershell"), Some(ShellArg::PowerShell));
        assert_eq!(parse_shell_name("pwsh"), Some(ShellArg::PowerShell));
        assert_eq!(parse_shell_name("/bin/bash"), None); // filename extraction done beforehand
        assert_eq!(parse_shell_name("unknown"), None);
    }

    #[test]
    fn filenames_match_shell_conventions() {
        assert_eq!(filename_for(ShellArg::Bash), "wright");
        assert_eq!(filename_for(ShellArg::Zsh), "_wright");
        assert_eq!(filename_for(ShellArg::Fish), "wright.fish");
        assert_eq!(filename_for(ShellArg::PowerShell), "_wright.ps1");
    }

    #[test]
    fn generate_script_produces_non_empty_content_for_all_shells() {
        for shell in [
            ShellArg::Bash,
            ShellArg::Zsh,
            ShellArg::Fish,
            ShellArg::PowerShell,
        ] {
            let script = generate_script(shell);
            assert!(!script.is_empty(), "{:?} script must not be empty", shell);
            let text = String::from_utf8_lossy(&script);
            assert!(text.contains("wright"), "{:?} script contains wright", shell);
        }
    }
}
