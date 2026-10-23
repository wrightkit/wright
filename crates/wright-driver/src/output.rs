use std::path::{Path, PathBuf};

use crate::diag::{Diagnostic, Stage};
use crate::input::display_path;

/// The program's destination.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum OutputDest {
    /// `<entry stem>.ow` in the current directory.
    #[default]
    Default,
    /// An explicit file.
    Path(PathBuf),
    /// The program stays in the in-process result for the caller to print.
    Stdout,
}

/// What to do when the destination file already exists.
pub enum Overwrite<'a> {
    /// Refuse with a usage error that names `--force`.
    Refuse,
    /// Replace without asking.
    Force,
    /// Ask the caller; `false` keeps the file.
    Ask(&'a mut dyn FnMut(&Path) -> bool),
}

/// How a compile delivers its program.
pub struct CompileWrite<'a> {
    pub dest: OutputDest,
    pub overwrite: Overwrite<'a>,
    /// Output locale override (`compile --to`): `None` keeps the loaded
    /// input's locale.
    pub to: Option<String>,
}

pub(crate) fn write_program(
    text: &str,
    entry: Option<&Path>,
    write: CompileWrite<'_>,
) -> Result<String, Diagnostic> {
    let path = match write.dest {
        OutputDest::Stdout => return Ok("stdout".to_string()),
        OutputDest::Path(path) => path,
        OutputDest::Default => default_path(entry)?,
    };
    if path.exists() {
        if let Some(entry) = entry
            && same_file(&path, entry)
        {
            return Err(Diagnostic::error(
                "output-is-input",
                Stage::Discovery,
                format!(
                    "refusing to overwrite the input '{}'; choose another path with -o",
                    display_path(&path)
                ),
            ));
        }
        let replace = match write.overwrite {
            Overwrite::Force => true,
            Overwrite::Refuse => {
                return Err(Diagnostic::error(
                    "output-exists",
                    Stage::Discovery,
                    format!(
                        "'{}' already exists; pass --force (agent request: force) to overwrite it, or choose another path with -o",
                        display_path(&path)
                    ),
                ));
            }
            Overwrite::Ask(ask) => ask(&path),
        };
        if !replace {
            return Err(Diagnostic::error(
                "output-declined",
                Stage::Discovery,
                format!("'{}' was not overwritten", display_path(&path)),
            ));
        }
    }
    std::fs::write(&path, text).map_err(|error| {
        Diagnostic::error(
            "output-io",
            Stage::Emission,
            format!("cannot write output '{}': {error}", path.display()),
        )
    })?;
    Ok(display_path(&path))
}

fn default_path(entry: Option<&Path>) -> Result<PathBuf, Diagnostic> {
    let name = entry
        .and_then(|entry| entry.canonicalize().ok())
        .and_then(|entry| {
            let part = if entry.is_dir() {
                entry.file_name()
            } else {
                entry.file_stem()
            };
            part.map(|name| name.to_string_lossy().into_owned())
        });
    match name {
        Some(name) => Ok(PathBuf::from(format!("{name}.ow"))),
        None => Err(Diagnostic::error(
            "output-unnamed",
            Stage::Discovery,
            "the input has no file name to derive the output name from; choose a path with -o"
                .to_string(),
        )),
    }
}

fn same_file(output: &Path, entry: &Path) -> bool {
    match (output.canonicalize(), entry.canonicalize()) {
        (Ok(output), Ok(entry)) => output == entry,
        _ => false,
    }
}
