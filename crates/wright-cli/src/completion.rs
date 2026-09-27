use std::path::{Path, PathBuf};

use clap::CommandFactory;

use crate::CLI_NAME;
use crate::cli::{Cli, CompletionInstallArgs, ShellArg};

mod exit {
    pub(super) const SUCCESS: u8 = 0;
    pub(super) const USER_ERROR: u8 = 1;
    pub(super) const USAGE: u8 = 2;
    pub(super) const INTERNAL: u8 = 4;
}

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

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum InstallStatus {
    Created(PathBuf),
    Updated(PathBuf),
    UpToDate(PathBuf),
    DryRun(PathBuf),
}

pub(crate) fn generate_script(shell: ShellArg) -> Vec<u8> {
    let mut command = Cli::command();
    let mut buffer = Vec::new();
    clap_complete::generate(shell.to_clap_shell(), &mut command, CLI_NAME, &mut buffer);
    buffer
}

pub(crate) fn filename_for(shell: ShellArg) -> &'static str {
    shell.metadata().1
}

fn env_var_non_empty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|val| !val.trim().is_empty())
}

pub(crate) fn detect_shell() -> Result<ShellArg, CompletionError> {
    if let Some(override_shell) = env_var_non_empty("WRIGHT_SHELL") {
        return parse_shell_name(&override_shell).ok_or_else(|| {
            CompletionError::usage(format!(
                "invalid shell '{override_shell}' in WRIGHT_SHELL (expected bash|zsh|fish|powershell)"
            ))
        });
    }

    if let Some(shell_path) = env_var_non_empty("SHELL") {
        let shell_name = Path::new(&shell_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&shell_path);
        if let Some(shell) = parse_shell_name(shell_name) {
            return Ok(shell);
        }
    }

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
        || cfg!(windows)
    {
        return Ok(ShellArg::PowerShell);
    }

    Err(CompletionError::undetected(
        "could not automatically detect your shell; specify it explicitly with `wright completion install <bash|zsh|fish|powershell>`",
    ))
}

fn parse_shell_name(name: &str) -> Option<ShellArg> {
    let lower = name.to_ascii_lowercase();
    let name = lower.trim();
    if name.starts_with("zsh") {
        Some(ShellArg::Zsh)
    } else if name.starts_with("bash") {
        Some(ShellArg::Bash)
    } else if name.starts_with("fish") {
        Some(ShellArg::Fish)
    } else if name.starts_with("pwsh") || name.starts_with("powershell") {
        Some(ShellArg::PowerShell)
    } else {
        None
    }
}

fn user_home_dir() -> Result<PathBuf, CompletionError> {
    std::env::var("HOME")
        .ok()
        .filter(|home| !home.is_empty())
        .or_else(|| std::env::var("USERPROFILE").ok().filter(|home| !home.is_empty()))
        .map(PathBuf::from)
        .ok_or_else(|| {
            CompletionError::failed(
                "could not determine user home directory (HOME or USERPROFILE environment variable not set)",
            )
        })
}

fn xdg_dir(home: &Path, var: &str, default_sub: &str) -> PathBuf {
    std::env::var(var)
        .ok()
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(default_sub))
}

fn completion_override_dir() -> Option<PathBuf> {
    std::env::var("WRIGHT_COMPLETION_DIR")
        .ok()
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
}

fn completion_dirs() -> Result<(PathBuf, PathBuf, PathBuf), CompletionError> {
    let home = user_home_dir()?;
    let data_home = xdg_dir(&home, "XDG_DATA_HOME", ".local/share");
    let config_home = xdg_dir(&home, "XDG_CONFIG_HOME", ".config");
    Ok((home, data_home, config_home))
}

pub(crate) fn default_dir_for(shell: ShellArg) -> Result<PathBuf, CompletionError> {
    if let Some(override_dir) = completion_override_dir() {
        return Ok(override_dir);
    }

    let (home, data_home, config_home) = completion_dirs()?;
    let candidates = completion_candidates(shell, &home, &data_home, &config_home);

    match shell {
        ShellArg::Fish | ShellArg::Bash => Ok(candidates
            .iter()
            .find(|dir| dir.is_dir())
            .cloned()
            .unwrap_or_else(|| candidates[0].clone())),
        ShellArg::Zsh => {
            let custom = std::env::var("ZSH_CUSTOM")
                .ok()
                .filter(|custom| !custom.is_empty());
            let custom_index = if let Some(custom) = custom {
                if candidates[0].is_dir() || PathBuf::from(custom).is_dir() {
                    return Ok(candidates[0].clone());
                }
                1
            } else {
                0
            };
            if candidates[custom_index].is_dir() {
                return Ok(candidates[custom_index].clone());
            }
            if home.join(".oh-my-zsh").is_dir() {
                return Ok(candidates[custom_index].clone());
            }
            Ok(candidates
                .iter()
                .skip(custom_index + 1)
                .find(|dir| dir.is_dir())
                .cloned()
                .unwrap_or_else(|| candidates.last().expect("zsh has a fallback").clone()))
        }
        ShellArg::PowerShell => {
            if cfg!(windows) {
                Ok(if candidates[1].is_dir() && !candidates[0].is_dir() {
                    candidates[1].clone()
                } else {
                    candidates[0].clone()
                })
            } else {
                Ok(candidates[2].clone())
            }
        }
    }
}

fn completion_candidates(
    shell: ShellArg,
    home: &Path,
    data_home: &Path,
    config_home: &Path,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    match shell {
        ShellArg::Fish => {
            dirs.push(config_home.join("fish/completions"));
            dirs.push(data_home.join("fish/vendor_completions.d"));
        }
        ShellArg::Bash => {
            dirs.push(data_home.join("bash-completion/completions"));
            dirs.push(home.join(".bash_completion.d"));
        }
        ShellArg::Zsh => {
            if let Ok(custom) = std::env::var("ZSH_CUSTOM") {
                if !custom.is_empty() {
                    dirs.push(PathBuf::from(custom).join("completions"));
                }
            }
            dirs.push(home.join(".oh-my-zsh/custom/completions"));
            dirs.push(home.join(".zfunc"));
            dirs.push(home.join(".zsh/completions"));
            dirs.push(data_home.join("zsh/site-functions"));
        }
        ShellArg::PowerShell => {
            dirs.push(home.join("Documents/PowerShell/Scripts"));
            dirs.push(home.join("Documents/WindowsPowerShell/Scripts"));
            dirs.push(config_home.join("powershell/completions"));
            dirs.push(data_home.join("powershell/Completions"));
        }
    }
    dirs
}

pub(crate) fn candidate_dirs_for(shell: ShellArg) -> Vec<PathBuf> {
    if let Some(override_dir) = completion_override_dir() {
        return vec![override_dir];
    }
    let Ok((home, data_home, config_home)) = completion_dirs() else {
        return Vec::new();
    };
    completion_candidates(shell, &home, &data_home, &config_home)
}

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

    let updating = target_file.is_file();
    if updating {
        let existing = std::fs::read(&target_file).map_err(|e| {
            CompletionError::failed(format!(
                "could not read existing completion file {}: {e}",
                target_file.display()
            ))
        })?;
        if existing == content && !force {
            return Ok(InstallStatus::UpToDate(target_file));
        }
    } else {
        std::fs::create_dir_all(&target_dir).map_err(|e| {
            CompletionError::failed(format!(
                "could not create completion directory {}: {e}",
                target_dir.display()
            ))
        })?;
    }
    std::fs::write(&target_file, &content).map_err(|e| {
        CompletionError::failed(format!(
            "could not {} completion file {}: {e}",
            if updating { "update" } else { "write" },
            target_file.display()
        ))
    })?;
    Ok(if updating {
        InstallStatus::Updated(target_file)
    } else {
        InstallStatus::Created(target_file)
    })
}

fn print_status(status: &InstallStatus, shell: ShellArg, guidance: bool) {
    match status {
        InstallStatus::Created(path) => {
            println!(
                "==> installed {} completion to {}",
                shell.as_str(),
                path.display()
            );
            if guidance {
                print_guidance(shell, path);
            }
        }
        InstallStatus::Updated(path) => {
            println!(
                "==> updated {} completion in {}",
                shell.as_str(),
                path.display()
            );
            if guidance {
                print_guidance(shell, path);
            }
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
            if guidance {
                print_guidance(shell, path);
            }
        }
    }
}

fn print_guidance(shell: ShellArg, target_file: &Path) {
    let target_dir = target_file.parent().unwrap_or(target_file);
    match shell {
        ShellArg::Zsh => {
            if target_dir.to_string_lossy().contains(".oh-my-zsh") {
                println!("note: completion installed into Oh My Zsh custom completions directory");
            } else {
                println!(
                    "note: ensure {} is in your zsh $fpath (e.g. fpath=({} $fpath) in ~/.zshrc)",
                    target_dir.display(),
                    target_dir.display()
                );
            }
        }
        ShellArg::Bash => println!(
            "note: bash completions in {} are loaded automatically when bash-completion is active",
            target_dir.display()
        ),
        ShellArg::Fish => println!(
            "note: fish autoloads completions from {}",
            target_dir.display()
        ),
        ShellArg::PowerShell => println!(
            "note: add '. \"{}\"' to your PowerShell $PROFILE if not already autoloaded",
            target_file.display()
        ),
    }
}

pub(crate) fn run_install(args: &CompletionInstallArgs) -> Result<u8, CompletionError> {
    let shells = if args.all {
        ShellArg::ALL.to_vec()
    } else {
        vec![match args.effective_shell() {
            Some(s) => s,
            None => detect_shell()?,
        }]
    };

    for shell in shells {
        let status = install_for_shell(shell, args.dir.as_deref(), args.dry_run, args.force)?;
        print_status(&status, shell, !args.all);
    }
    Ok(exit::SUCCESS)
}

pub(crate) fn refresh_installed_completions() -> Result<usize, String> {
    let mut refreshed = 0;
    for shell in ShellArg::ALL {
        let filename = filename_for(shell);
        for dir in candidate_dirs_for(shell) {
            let target_file = dir.join(filename);
            if target_file.is_file() {
                match install_for_shell(shell, Some(&dir), false, false) {
                    Ok(InstallStatus::Updated(path) | InstallStatus::Created(path)) => {
                        println!(
                            "==> refreshed {} completion in {}",
                            shell.as_str(),
                            path.display()
                        );
                        refreshed += 1;
                    }
                    Ok(InstallStatus::UpToDate(_) | InstallStatus::DryRun(_)) => {}
                    Err(error) => {
                        eprintln!(
                            "warning: could not refresh {} completion in {}: {}",
                            shell.as_str(),
                            target_file.display(),
                            error.message()
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
            assert!(
                text.contains("wright"),
                "{:?} script contains wright",
                shell
            );
        }
    }
}
