//! Local artifact-store plumbing shared by `opy_provider` and
//! `lint_package`: the platform data dir, staging-directory reservation,
//! atomic promotion, and the bounded reqwest reads their downloads use.

use std::path::{Path, PathBuf};

/// The Wright platform data dir — `$HOME/Library/Application
/// Support/wright` on macOS, `%APPDATA%/wright` on Windows, and
/// `$XDG_DATA_HOME/wright` or `$HOME/.local/wright` elsewhere — before the
/// caller's own subdirectory tail.
pub(crate) fn wright_data_base() -> PathBuf {
    let base = match std::env::consts::OS {
        "macos" => std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|path| path.join("Library").join("Application Support")),
        "windows" => std::env::var_os("APPDATA").map(PathBuf::from),
        _ => std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|path| PathBuf::from(path).join(".local"))),
    };
    base.unwrap_or_else(|| PathBuf::from(".")).join("wright")
}

/// Reserve a fresh `.{prefix}-<pid>-<nanos>-<attempt>` directory under
/// `root`, retrying name collisions through `0..1024`. `noun` names what
/// is being staged, article included, for the reservation failure ("an
/// OPY provider", "a lint package"); the create error drops the article.
pub fn reserve_staging_dir(root: &Path, prefix: &str, noun: &str) -> Result<PathBuf, String> {
    let bare = noun
        .strip_prefix("an ")
        .or_else(|| noun.strip_prefix("a "))
        .unwrap_or(noun);
    for attempt in 0..1024 {
        let staging = root.join(format!(
            "{prefix}-{}-{}-{attempt}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        match std::fs::create_dir(&staging) {
            Ok(()) => return Ok(staging),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(format!(
                    "cannot create {bare} staging directory '{}': {e}",
                    staging.display()
                ));
            }
        }
    }
    Err(format!(
        "cannot reserve {noun} staging directory in '{}'",
        root.display()
    ))
}

/// The step a [`promote_dir`] call failed on, so each caller words its own
/// diagnostic.
pub(crate) enum PromoteError {
    /// `final_dir`'s parent could not be created.
    CreateParent(std::io::Error),
    /// `staging` could not be renamed and the destination is not intact.
    Rename(std::io::Error),
}

/// Rename `staging` into place as `final_dir`, creating the destination's
/// parent first. A failed rename still succeeds when `intact` reports the
/// destination already holds a complete entry — a concurrent install won
/// the race.
pub(crate) fn promote_dir(
    staging: &Path,
    final_dir: &Path,
    intact: impl FnOnce() -> bool,
) -> Result<(), PromoteError> {
    std::fs::create_dir_all(final_dir.parent().expect("promotion target has a parent"))
        .map_err(PromoteError::CreateParent)?;
    match std::fs::rename(staging, final_dir) {
        Ok(()) => Ok(()),
        Err(_) if intact() => Ok(()),
        Err(e) => Err(PromoteError::Rename(e)),
    }
}

/// The HTTPS client every bundled fetcher uses: the caller's user agent
/// and a 60-second timeout.
pub fn client(agent: &str) -> Result<reqwest::blocking::Client, reqwest::Error> {
    reqwest::blocking::Client::builder()
        .user_agent(agent)
        .timeout(std::time::Duration::from_secs(60))
        .build()
}

/// The way a [`read_limited`] call failed, so each caller words its own
/// diagnostic.
#[derive(Debug)]
pub enum ReadError {
    /// The response body could not be read.
    Io(std::io::Error),
    /// The body exceeded the caller's byte limit.
    OverLimit,
}

/// Read a response body capped at `limit` bytes: one byte past the limit
/// reports [`ReadError::OverLimit`] instead of a truncated success.
pub fn read_limited(
    response: reqwest::blocking::Response,
    limit: u64,
) -> Result<Vec<u8>, ReadError> {
    use std::io::Read;
    let mut body = Vec::new();
    response
        .take(limit + 1)
        .read_to_end(&mut body)
        .map_err(ReadError::Io)?;
    if body.len() as u64 > limit {
        return Err(ReadError::OverLimit);
    }
    Ok(body)
}

/// The way a [`verify_sha256`] call failed, so each caller words its own
/// diagnostic.
#[derive(Debug)]
pub enum ChecksumError {
    /// The checksum file held no digest.
    Empty,
    /// The first field was not a 64-digit SHA-256 hex digest.
    Invalid(String),
    /// The published digest did not match the archive. Carries
    /// `(published, actual)`.
    Mismatch(String, String),
}

/// The one SHA-256 checksum verification used by every download: the first
/// whitespace-separated field of `checksum_file` must be a 64-digit hex
/// digest matching the SHA-256 of `archive`.
pub fn verify_sha256(archive: &[u8], checksum_file: &str) -> Result<(), ChecksumError> {
    let published = checksum_file
        .split_whitespace()
        .next()
        .ok_or(ChecksumError::Empty)?;
    if published.len() != 64 || !published.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ChecksumError::Invalid(published.to_string()));
    }
    let actual = crate::input::sha256_hex(archive);
    if !actual.eq_ignore_ascii_case(published) {
        return Err(ChecksumError::Mismatch(published.to_string(), actual));
    }
    Ok(())
}
