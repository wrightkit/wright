//! Provides the authoritative lifecycle for shell completions generated from
//! the `clap` command model. Supports pure generation (`wright completion <shell>`),
//! automatic or explicit installation into conventional user-local locations
//! (`wright completion install [shell]`), and idempotent refresh during `wright update`.

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

/// A failure of completion operations.
#[derive(Debug)]
pub(crate) struct CompletionError {
    code: u8,
    message: String,
}

impl CompletionError {
    pub(crate) fn usage(m: impl Into<String>) -> Self {
        Self {
            code: exit::USAGE,
            message: m.into(),
        }
    }
    pub(crate) fn undetected(m: impl Into<String>) -> Self {
        Self {
            code: exit::USER_ERROR,
            message: m.into(),
        }
    }
    pub(crate) fn failed(m: impl Into<String>) -> Self {
        Self {
            code: exit::INTERNAL,
            message: m.into(),
        }
    }

    pub(crate) fn exit_code(&self) -> u8 {
        self.code
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
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
    pub(crate) fn path(&self) -> &Path {
        match self {
            InstallStatus::Created(p)
            | InstallStatus::Updated(p)
            | InstallStatus::UpToDate(p)
            | InstallStatus::DryRun(p) => p,
        }
    }

    fn report(&self, shell: ShellArg) {
        let s = shell.as_str();
        let p = self.path().display();
        match self {
            Self::Created(_) => println!("==> installed {s} completion to {p}"),
            Self::Updated(_) => println!("==> updated {s} completion in {p}"),
            Self::UpToDate(_) => println!("{s} completion in {p} is already up to date"),
            Self::DryRun(_) => println!("would install {s} completion to {p}"),
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

    if let Some(shell_path) = env_var_non_empty("SHELL") {
        let shell_name = Path::new(&shell_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&shell_path);
        if let Some(shell) = parse_shell_name(shell_name) {
            return Ok(shell);
        }
    }

    const SHELL_VARS: &[(&str, ShellArg)] = &[
        ("ZSH_VERSION", ShellArg::Zsh),
        ("ZDOTDIR", ShellArg::Zsh),
        ("BASH_VERSION", ShellArg::Bash),
        ("FISH_VERSION", ShellArg::Fish),
        ("PSModulePath", ShellArg::PowerShell),
        ("POWERSHELL_DISTRIBUTION_CHANNEL", ShellArg::PowerShell),
        ("PSExecutionPolicyPreference", ShellArg::PowerShell),
    ];
    for &(var, shell) in SHELL_VARS {
        if env_var_non_empty(var).is_some() {
            return Ok(shell);
        }
    }

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
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
        .filter(|val| !val.trim().is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            CompletionError::failed(
                "could not determine user home directory (HOME or USERPROFILE environment variable not set)",
            )
        })
}

fn xdg_dir(var: &str, fallback: impl FnOnce(&Path) -> PathBuf) -> Result<PathBuf, CompletionError> {
    env_var_non_empty(var)
        .map(PathBuf::from)
        .map(Ok)
        .unwrap_or_else(|| user_home_dir().map(|h| fallback(&h)))
}

fn xdg_data_home() -> Result<PathBuf, CompletionError> {
    xdg_dir("XDG_DATA_HOME", |h| h.join(".local").join("share"))
}

fn xdg_config_home() -> Result<PathBuf, CompletionError> {
    xdg_dir("XDG_CONFIG_HOME", |h| h.join(".config"))
}

/// Determine the default conventional installation directory for a given shell.
pub(crate) fn default_dir_for(shell: ShellArg) -> Result<PathBuf, CompletionError> {
    if let Some(override_dir) = env_var_non_empty("WRIGHT_COMPLETION_DIR") {
        return Ok(PathBuf::from(override_dir));
    }

    let home = user_home_dir()?;
    let data_home = xdg_data_home()?;
    let config_home = xdg_config_home()?;

    match shell {
        ShellArg::Fish => {
            let (cfg, vdr) = (
                config_home.join("fish").join("completions"),
                data_home.join("fish").join("vendor_completions.d"),
            );
            Ok(if vdr.is_dir() && !cfg.is_dir() {
                vdr
            } else {
                cfg
            })
        }
        ShellArg::Bash => {
            let (xdg, leg) = (
                data_home.join("bash-completion").join("completions"),
                home.join(".bash_completion.d"),
            );
            Ok(if leg.is_dir() && !xdg.is_dir() {
                leg
            } else {
                xdg
            })
        }
        ShellArg::Zsh => {
            if let Some(custom) = env_var_non_empty("ZSH_CUSTOM") {
                let p = PathBuf::from(custom);
                let c = p.join("completions");
                if c.is_dir() || p.is_dir() {
                    return Ok(c);
                }
            }
            let omz = home.join(".oh-my-zsh").join("custom").join("completions");
            if omz.is_dir() || home.join(".oh-my-zsh").is_dir() {
                return Ok(omz);
            }
            for c in [home.join(".zfunc"), home.join(".zsh").join("completions")] {
                if c.is_dir() {
                    return Ok(c);
                }
            }
            Ok(data_home.join("zsh").join("site-functions"))
        }
        ShellArg::PowerShell => {
            if cfg!(windows) {
                let (ps, win) = (
                    home.join("Documents").join("PowerShell").join("Scripts"),
                    home.join("Documents")
                        .join("WindowsPowerShell")
                        .join("Scripts"),
                );
                Ok(if win.is_dir() && !ps.is_dir() {
                    win
                } else {
                    ps
                })
            } else {
                Ok(config_home.join("powershell").join("completions"))
            }
        }
    }
}

/// Return all candidate conventional directories for a given shell.
pub(crate) fn candidate_dirs_for(shell: ShellArg) -> Vec<PathBuf> {
    if let Some(override_dir) = env_var_non_empty("WRIGHT_COMPLETION_DIR") {
        return vec![PathBuf::from(override_dir)];
    }
    let (Ok(home), Ok(data_home), Ok(config_home)) =
        (user_home_dir(), xdg_data_home(), xdg_config_home())
    else {
        return Vec::new();
    };

    match shell {
        ShellArg::Fish => vec![
            config_home.join("fish").join("completions"),
            data_home.join("fish").join("vendor_completions.d"),
        ],
        ShellArg::Bash => vec![
            data_home.join("bash-completion").join("completions"),
            home.join(".bash_completion.d"),
        ],
        ShellArg::Zsh => {
            let mut dirs = env_var_non_empty("ZSH_CUSTOM")
                .map(|c| PathBuf::from(c).join("completions"))
                .into_iter()
                .collect::<Vec<_>>();
            dirs.extend([
                home.join(".oh-my-zsh").join("custom").join("completions"),
                home.join(".zfunc"),
                home.join(".zsh").join("completions"),
                data_home.join("zsh").join("site-functions"),
            ]);
            dirs
        }
        ShellArg::PowerShell => vec![
            home.join("Documents").join("PowerShell").join("Scripts"),
            home.join("Documents")
                .join("WindowsPowerShell")
                .join("Scripts"),
            config_home.join("powershell").join("completions"),
            data_home.join("powershell").join("Completions"),
        ],
    }
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

    let exists = target_file.is_file();
    if exists {
        if !force
            && std::fs::read(&target_file)
                .map(|e| e == content)
                .unwrap_or(false)
        {
            return Ok(InstallStatus::UpToDate(target_file));
        }
    } else {
        std::fs::create_dir_all(&target_dir).map_err(|err| {
            CompletionError::failed(format!(
                "could not create completion directory {}: {err}",
                target_dir.display()
            ))
        })?;
    }

    std::fs::write(&target_file, &content).map_err(|err| {
        let op = if exists { "update" } else { "write" };
        CompletionError::failed(format!(
            "could not {op} completion file {}: {err}",
            target_file.display()
        ))
    })?;
    Ok(if exists {
        InstallStatus::Updated(target_file)
    } else {
        InstallStatus::Created(target_file)
    })
}

fn print_guidance(shell: ShellArg, target_file: &Path) {
    let dir = target_file.parent().unwrap_or(target_file).display();
    match shell {
        ShellArg::Zsh if dir.to_string().contains(".oh-my-zsh") => {
            println!("note: completion installed into Oh My Zsh custom completions directory");
        }
        ShellArg::Zsh => println!(
            "note: ensure {dir} is in your zsh $fpath (e.g. fpath=({dir} $fpath) in ~/.zshrc)"
        ),
        ShellArg::Bash => println!(
            "note: bash completions in {dir} are loaded automatically when bash-completion is active"
        ),
        ShellArg::Fish => println!("note: fish autoloads completions from {dir}"),
        ShellArg::PowerShell => println!(
            "note: add '. \"{}\"' to your PowerShell $PROFILE if not already autoloaded",
            target_file.display()
        ),
    }
}

/// Run the `completion install` workflow.
pub(crate) fn run_install(args: &CompletionInstallArgs) -> Result<u8, CompletionError> {
    let shells: Vec<ShellArg> = if args.all {
        vec![
            ShellArg::Bash,
            ShellArg::Zsh,
            ShellArg::Fish,
            ShellArg::PowerShell,
        ]
    } else {
        vec![match args.effective_shell() {
            Some(s) => s,
            None => detect_shell()?,
        }]
    };

    for &shell in &shells {
        let status = install_for_shell(shell, args.dir.as_deref(), args.dry_run, args.force)?;
        status.report(shell);
        if !args.all && !matches!(status, InstallStatus::UpToDate(_)) {
            print_guidance(shell, status.path());
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
                    Ok(InstallStatus::Updated(p) | InstallStatus::Created(p)) => {
                        println!(
                            "==> refreshed {} completion in {}",
                            shell.as_str(),
                            p.display()
                        );
                        refreshed += 1;
                    }
                    Ok(InstallStatus::UpToDate(_) | InstallStatus::DryRun(_)) => {}
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
        for (input, expected) in [
            ("bash", Some(ShellArg::Bash)),
            ("zsh", Some(ShellArg::Zsh)),
            ("fish", Some(ShellArg::Fish)),
            ("powershell", Some(ShellArg::PowerShell)),
            ("pwsh", Some(ShellArg::PowerShell)),
            ("/bin/bash", None),
            ("unknown", None),
        ] {
            assert_eq!(parse_shell_name(input), expected);
        }
    }

    #[test]
    fn filenames_match_shell_conventions() {
        for (shell, expected) in [
            (ShellArg::Bash, "wright"),
            (ShellArg::Zsh, "_wright"),
            (ShellArg::Fish, "wright.fish"),
            (ShellArg::PowerShell, "_wright.ps1"),
        ] {
            assert_eq!(filename_for(shell), expected);
        }
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
            assert!(
                String::from_utf8_lossy(&script).contains("wright"),
                "{:?} script contains wright",
                shell
            );
        }
    }
}
