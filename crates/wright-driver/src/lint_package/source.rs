//! Lint package source identity: a GitHub `owner/repo` pair, with parsing for
//! the repository URL spellings users give (https, ssh, `git@`, and URLs
//! addressing a ref such as `/tree/<branch>` or `/releases/tag/<tag>`).

use std::fmt;

use super::LintPackageError;

/// A GitHub `owner/repo` pair naming one lint rule package source.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LintPackageSource {
    owner: String,
    repo: String,
}

impl LintPackageSource {
    /// An `owner`/`repo` pair under GitHub's name rules.
    pub fn new(owner: &str, repo: &str) -> Result<Self, LintPackageError> {
        if !valid_owner(owner) {
            return Err(LintPackageError::source(format!(
                "invalid GitHub repository owner '{owner}'"
            )));
        }
        if !valid_repo(repo) {
            return Err(LintPackageError::source(format!(
                "invalid GitHub repository name '{repo}'"
            )));
        }
        Ok(Self {
            owner: owner.to_string(),
            repo: repo.to_string(),
        })
    }

    /// Parse `owner/repo`, a `github.com` repository URL (https, `ssh://`, or
    /// `git@`), or a URL addressing a ref (`/tree/<ref>`, `/commit/<ref>`,
    /// `/releases/tag/<ref>`, `/blob/<ref>/…`). Returns the source plus the
    /// ref carried by the URL, when any.
    pub fn parse(input: &str) -> Result<(Self, Option<String>), LintPackageError> {
        let input = input.trim();
        if let Some(path) = input.strip_prefix("git@github.com:") {
            return Self::parse_path(path, input);
        }
        if let Some(path) = input.strip_prefix("ssh://git@github.com/") {
            return Self::parse_path(path, input);
        }
        if input.contains("://")
            || input.starts_with("github.com/")
            || input.starts_with("www.github.com/")
        {
            let text = if input.contains("://") {
                input.to_string()
            } else {
                format!("https://{input}")
            };
            let url = url::Url::parse(&text).map_err(|_| Self::invalid_input(input))?;
            if !matches!(url.host_str(), Some("github.com" | "www.github.com")) {
                return Err(LintPackageError::source(format!(
                    "lint rule packages are fetched from github.com repositories, not '{input}'"
                )));
            }
            return Self::parse_path(url.path().trim_start_matches('/'), input);
        }
        Self::parse_path(input, input)
    }

    fn invalid_input(input: &str) -> LintPackageError {
        LintPackageError::source(format!(
            "invalid lint package source '{input}': expected 'owner/repo' or a github.com repository URL"
        ))
    }

    fn parse_path(path: &str, input: &str) -> Result<(Self, Option<String>), LintPackageError> {
        let segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
        let invalid = || Self::invalid_input(input);
        let (owner, repo) = match segments.as_slice() {
            [owner, repo, ..] => (*owner, *repo),
            _ => return Err(invalid()),
        };
        let reference = match &segments[2..] {
            [] => None,
            ["tree" | "commit", reference @ ..] if !reference.is_empty() => {
                Some(reference.join("/"))
            }
            ["blob", reference, ..] => Some((*reference).to_string()),
            ["releases", "tag", reference @ ..] if !reference.is_empty() => {
                Some(reference.join("/"))
            }
            _ => return Err(invalid()),
        };
        let repo = repo.strip_suffix(".git").unwrap_or(repo);
        Ok((Self::new(owner, repo)?, reference))
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn repo(&self) -> &str {
        &self.repo
    }

    /// The `owner/repo` identity used in diagnostics and result records.
    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }
}

impl fmt::Display for LintPackageSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.repo)
    }
}

/// GitHub usernames and organization names are ASCII alphanumerics and
/// interior hyphens.
fn valid_owner(owner: &str) -> bool {
    !owner.is_empty()
        && owner.len() <= 39
        && !owner.starts_with('-')
        && !owner.ends_with('-')
        && owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn valid_repo(repo: &str) -> bool {
    !repo.is_empty()
        && repo.len() <= 100
        && repo != "."
        && repo != ".."
        && repo
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}
