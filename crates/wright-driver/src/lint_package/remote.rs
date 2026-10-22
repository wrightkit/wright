//! The remote half of the lint package store: commits-API resolution with
//! ETag conditional requests, and the `codeload.github.com` tarball fetch.
//!
//! `GITHUB_TOKEN` is attached to API requests only — never to codeload —
//! because a codeload tarball for a full SHA does not count against the API
//! rate limit and the token must not leave the API host.

use std::collections::HashMap;
use std::io::Read;
use std::sync::Mutex;
use std::time::Duration;
use wright_analyzer::service::marker;

use super::{LintPackageError, LintPackageSource};

pub(super) const MAX_DOWNLOAD_BYTES: u64 = 128 * 1024 * 1024;
pub(super) const TOKEN_ENV: &str = "GITHUB_TOKEN";

/// Ref spellings that may appear in a URL path keep `/`, `.`, `-`, and `_`
/// unescaped so branch names such as `feature/x` reach the commits API whole.
const REF_ENCODE: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'/')
    .remove(b'.')
    .remove(b'-')
    .remove(b'_');

/// Query-string spellings keep the search qualifier colon plus `.`, `-`,
/// and `_` readable; spaces and other characters are percent-encoded.
const QUERY_ENCODE: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b':')
    .remove(b'.')
    .remove(b'-')
    .remove(b'_');

/// An ETag-conditional resolution cache: request URL → the entity tag GitHub
/// issued and the response body it was issued for.
pub(super) type ResolutionCache = Mutex<HashMap<String, ResolvedEntry>>;

#[derive(Debug, Clone)]
pub(super) struct ResolvedEntry {
    etag: String,
    body: Vec<u8>,
}

/// The commits-API endpoint that resolves `reference`: `commits/<ref>` for a
/// tag/branch/short SHA, or `commits?per_page=1` for the default-branch head.
/// The boolean reports whether the answer is a single commit object.
pub(super) fn resolve_url(api_base: &str, source: &LintPackageSource, reference: &str) -> String {
    format!(
        "{}/repos/{}/{}/commits/{}",
        api_base.trim_end_matches('/'),
        source.owner(),
        source.repo(),
        percent_encoding::utf8_percent_encode(reference, REF_ENCODE)
    )
}

pub(super) fn default_head_url(api_base: &str, source: &LintPackageSource) -> String {
    format!(
        "{}/repos/{}/{}/commits?per_page=1",
        api_base.trim_end_matches('/'),
        source.owner(),
        source.repo()
    )
}

pub(super) fn archive_url(codeload_base: &str, source: &LintPackageSource, sha: &str) -> String {
    format!(
        "{}/{}/{}/tar.gz/{sha}",
        codeload_base.trim_end_matches('/'),
        source.owner(),
        source.repo()
    )
}

/// The repository Search API endpoint: the `wrightkit-lint` topic filter
/// plus optional caller terms, paged at `per_page=100` (1-based `page`).
pub(super) fn search_url(api_base: &str, terms: Option<&str>, page: u32) -> String {
    let mut query = "topic:wrightkit-lint archived:false fork:false".to_string();
    if let Some(terms) = terms.map(str::trim).filter(|t| !t.is_empty()) {
        query.push(' ');
        query.push_str(terms);
    }
    format!(
        "{}/search/repositories?q={}&per_page=100&page={page}",
        api_base.trim_end_matches('/'),
        percent_encoding::utf8_percent_encode(&query, QUERY_ENCODE)
    )
}

/// One parsed search page: `total_count`, `incomplete_results`, and the
/// raw `items` array.
pub(super) struct SearchPage {
    pub(super) total: u64,
    pub(super) incomplete: bool,
    pub(super) items: Vec<serde_json::Value>,
}

/// Parse one Search API page; a malformed or non-object answer is a
/// `lint-package-remote` error, matching `resolved_sha`.
pub(super) fn search_page(body: &[u8]) -> Result<SearchPage, LintPackageError> {
    let unexpected =
        || LintPackageError::remote("GitHub returned an unexpected repository search response");
    let value: serde_json::Value = serde_json::from_slice(body).map_err(|_| unexpected())?;
    let total = value["total_count"].as_u64().ok_or_else(unexpected)?;
    let items = value["items"].as_array().ok_or_else(unexpected)?.clone();
    Ok(SearchPage {
        total,
        incomplete: value["incomplete_results"].as_bool().unwrap_or(false),
        items,
    })
}

pub(super) fn package_client() -> Result<reqwest::blocking::Client, LintPackageError> {
    reqwest::blocking::Client::builder()
        .user_agent(concat!("wright-lint-packages/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| {
            LintPackageError::offline(format!(
                "cannot initialize HTTPS client for lint package fetch: {}",
                marker::escape(&e.to_string())
            ))
        })
}

/// One conditional GET against the GitHub API host. The token is attached
/// here — the only place requests carry it. A stored ETag goes out as
/// `If-None-Match`; GitHub's 304 reuses the stored body without another
/// counted request.
pub(super) fn api_get(
    client: &reqwest::blocking::Client,
    url: &str,
    token: Option<&str>,
    cache: &ResolutionCache,
) -> Result<Vec<u8>, LintPackageError> {
    let cached = cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(url)
        .cloned();
    let mut request = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    if let Some(cached) = &cached {
        request = request.header(reqwest::header::IF_NONE_MATCH, &cached.etag);
    }
    let response = request.send().map_err(|error| {
        LintPackageError::offline(format!(
            "cannot reach GitHub API {}: {}",
            marker::escape(url),
            marker::escape(&error.to_string())
        ))
    })?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = read_body(response, url)?;
    if status == reqwest::StatusCode::NOT_MODIFIED {
        return cached.map(|entry| entry.body).ok_or_else(|| {
            LintPackageError::remote(format!(
                "GitHub answered 304 Not Modified for {} without a stored response",
                marker::escape(url)
            ))
        });
    }
    if rate_limited(status, &headers, &body) {
        return Err(rate_limit_error(&headers, token.is_some()));
    }
    if !status.is_success() {
        return Err(classify_api_status(status, url));
    }
    let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
    match etag(&headers) {
        Some(etag) => {
            cache.insert(
                url.to_string(),
                ResolvedEntry {
                    etag,
                    body: body.clone(),
                },
            );
        }
        None => {
            cache.remove(url);
        }
    }
    drop(cache);
    Ok(body)
}

/// The codeload tarball for a full SHA. The token never leaves the API host,
/// so this request is unauthenticated by design.
pub(super) fn fetch_archive(
    client: &reqwest::blocking::Client,
    url: &str,
    source: &LintPackageSource,
    sha: &str,
    token_in_use: bool,
) -> Result<Vec<u8>, LintPackageError> {
    let response = client.get(url).send().map_err(|error| {
        LintPackageError::offline(format!(
            "cannot reach lint package archive {}: {}",
            marker::escape(url),
            marker::escape(&error.to_string())
        ))
    })?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = read_body(response, url)?;
    if rate_limited(status, &headers, &body) {
        return Err(rate_limit_error(&headers, token_in_use));
    }
    if !status.is_success() {
        return Err(classify_fetch_status(status, source, sha, url));
    }
    Ok(body)
}

/// The `sha` member of a single-commit object or of the first entry of the
/// commit list, normalized to lowercase. An empty commit list means the
/// repository exists but has no commits — a missing source, not a bad answer.
pub(super) fn resolved_sha(
    body: &[u8],
    single: bool,
    source: &LintPackageSource,
) -> Result<String, LintPackageError> {
    let unexpected = || {
        LintPackageError::remote(format!(
            "GitHub returned an unexpected commits response for '{}'",
            marker::escape(&source.to_string())
        ))
    };
    let value: serde_json::Value = serde_json::from_slice(body).map_err(|_| unexpected())?;
    let commit = if single {
        value
    } else {
        let list = value.as_array().ok_or_else(unexpected)?;
        match list.first() {
            Some(commit) => commit.clone(),
            None => {
                return Err(LintPackageError::not_found(format!(
                    "GitHub repository '{}' has no commits",
                    marker::escape(&source.to_string())
                )));
            }
        }
    };
    let sha = commit.get("sha").and_then(|sha| sha.as_str());
    match sha {
        Some(sha) if super::is_full_sha(sha) => Ok(sha.to_lowercase()),
        _ => Err(unexpected()),
    }
}

fn etag(headers: &reqwest::header::HeaderMap) -> Option<String> {
    headers
        .get(reqwest::header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn read_body(
    response: reqwest::blocking::Response,
    url: &str,
) -> Result<Vec<u8>, LintPackageError> {
    let mut body = Vec::new();
    response
        .take(MAX_DOWNLOAD_BYTES + 1)
        .read_to_end(&mut body)
        .map_err(|error| {
            LintPackageError::remote(format!(
                "cannot read the response from {}: {}",
                marker::escape(url),
                marker::escape(&error.to_string())
            ))
        })?;
    if body.len() as u64 > MAX_DOWNLOAD_BYTES {
        return Err(LintPackageError::remote(format!(
            "response from {} exceeds the 128 MiB limit",
            marker::escape(url)
        )));
    }
    Ok(body)
}

/// GitHub reports a used-up rate limit as 403 with `x-ratelimit-remaining: 0`
/// (or a "rate limit" body for secondary limits) and as 429.
fn rate_limited(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: &[u8],
) -> bool {
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return true;
    }
    if status != reqwest::StatusCode::FORBIDDEN {
        return false;
    }
    headers
        .get("x-ratelimit-remaining")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.trim() == "0")
        || String::from_utf8_lossy(&body[..body.len().min(2048)]).contains("rate limit")
}

fn rate_limit_error(headers: &reqwest::header::HeaderMap, token: bool) -> LintPackageError {
    let retry = headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .map(|seconds| format!("; retry after {seconds} seconds"))
        .unwrap_or_default();
    let hint = if token {
        "retry later".to_string()
    } else {
        format!("set {TOKEN_ENV} to raise the limit")
    };
    LintPackageError::rate_limited(format!("GitHub API rate limit exceeded{retry}; {hint}"))
}

fn classify_api_status(status: reqwest::StatusCode, url: &str) -> LintPackageError {
    match status {
        reqwest::StatusCode::UNAUTHORIZED => LintPackageError::remote(format!(
            "GitHub refused the request to {} (HTTP 401); check that {TOKEN_ENV} is valid",
            marker::escape(url)
        )),
        reqwest::StatusCode::NOT_FOUND
        | reqwest::StatusCode::GONE
        | reqwest::StatusCode::UNPROCESSABLE_ENTITY => LintPackageError::not_found(format!(
            "GitHub repository or ref not found at {} (HTTP {status}); the repository may be \
             private — set {TOKEN_ENV} to read private repositories",
            marker::escape(url)
        )),
        _ => LintPackageError::remote(format!(
            "GitHub API request to {} failed with HTTP {status}",
            marker::escape(url)
        )),
    }
}

fn classify_fetch_status(
    status: reqwest::StatusCode,
    source: &LintPackageSource,
    sha: &str,
    url: &str,
) -> LintPackageError {
    match status {
        reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::GONE => {
            LintPackageError::not_found(format!(
                "lint package '{}' has no archive at commit {sha} (HTTP {status}); the \
                 repository or SHA may not exist, or the repository is private — set {TOKEN_ENV} \
                 to read private repositories",
                marker::escape(&source.to_string())
            ))
        }
        _ => LintPackageError::remote(format!(
            "lint package archive request to {} failed with HTTP {status}",
            marker::escape(url)
        )),
    }
}
