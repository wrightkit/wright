use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use flate2::read::GzDecoder;

use super::{MAX_DOWNLOAD_BYTES, OpyProviderError, is_executable, provider_binary};

pub(super) fn provider_client() -> Result<reqwest::blocking::Client, OpyProviderError> {
    reqwest::blocking::Client::builder()
        .user_agent(concat!("wright-opy-provider/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| {
            OpyProviderError::offline(format!(
                "cannot initialize HTTPS client for OPY provider release source: {}",
                e
            ))
        })
}

pub(super) fn fetch_text(
    client: &reqwest::blocking::Client,
    url: &str,
) -> Result<String, OpyProviderError> {
    let bytes = fetch(client, url)?;
    String::from_utf8(bytes).map_err(|e| {
        OpyProviderError::download(format!("cannot decode response from {}: {}", url, e))
    })
}

pub(super) fn fetch(
    client: &reqwest::blocking::Client,
    url: &str,
) -> Result<Vec<u8>, OpyProviderError> {
    let response = client.get(url).send().map_err(|error| {
        OpyProviderError::offline(format!(
            "cannot reach OPY provider release source {}: {}",
            url, error
        ))
    })?;
    if !response.status().is_success() {
        return Err(OpyProviderError::download(format!(
            "GET {} failed with HTTP {}",
            url,
            response.status()
        )));
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_DOWNLOAD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| {
            OpyProviderError::download(format!("cannot read response from {}: {}", url, e))
        })?;
    if bytes.len() as u64 > MAX_DOWNLOAD_BYTES {
        return Err(OpyProviderError::download(format!(
            "response from {} exceeds the 128 MiB provider artifact limit",
            url
        )));
    }
    Ok(bytes)
}

pub(super) fn verify_checksum(
    archive: &[u8],
    checksum_file: &str,
    archive_name: &str,
) -> Result<(), OpyProviderError> {
    let published = checksum_file.split_whitespace().next().ok_or_else(|| {
        OpyProviderError::integrity(format!("empty checksum file for {}", archive_name))
    })?;
    if published.len() != 64 || !published.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(OpyProviderError::integrity(format!(
            "invalid SHA-256 checksum for {}",
            archive_name
        )));
    }
    let expected = published;
    let actual = crate::input::sha256_hex(archive);
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(OpyProviderError::integrity(format!(
            "SHA-256 verification failed for {} (published {expected}, got {actual}); the active provider was not changed",
            archive_name
        )));
    }
    Ok(())
}

pub(super) fn extract_provider(
    archive: &[u8],
    dest: &Path,
    target: &str,
) -> Result<PathBuf, OpyProviderError> {
    let binary = provider_binary(target);
    let decoder = GzDecoder::new(archive);
    let mut archive = tar::Archive::new(decoder);
    let entries = archive.entries().map_err(|error| {
        OpyProviderError::install(format!("cannot read OPY provider archive: {}", error))
    })?;
    let mut found = false;
    for entry in entries {
        let mut entry = entry.map_err(|error| {
            OpyProviderError::install(format!("cannot read OPY provider archive entry: {}", error))
        })?;
        let path = entry.path().map_err(|error| {
            OpyProviderError::install(format!(
                "cannot inspect OPY provider archive path: {}",
                error
            ))
        })?;
        if path != Path::new(binary) {
            return Err(OpyProviderError::install(
                "OPY provider archive contains an unexpected path",
            ));
        }
        if !entry.header().entry_type().is_file() {
            return Err(OpyProviderError::install(
                "OPY provider archive executable is not a regular file",
            ));
        }
        let output = dest.join(binary);
        let mut file = std::fs::File::create(&output).map_err(|error| {
            OpyProviderError::install(format!(
                "cannot create staged OPY provider '{}': {}",
                output.display(),
                error
            ))
        })?;
        std::io::copy(&mut entry, &mut file).map_err(|error| {
            OpyProviderError::install(format!(
                "cannot unpack staged OPY provider '{}': {}",
                output.display(),
                error
            ))
        })?;
        file.sync_all().map_err(|error| {
            OpyProviderError::install(format!(
                "cannot persist staged OPY provider '{}': {}",
                output.display(),
                error
            ))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = entry.header().mode().unwrap_or(0o755) | 0o111;
            std::fs::set_permissions(&output, std::fs::Permissions::from_mode(mode)).map_err(
                |error| {
                    OpyProviderError::install(format!(
                        "cannot make staged OPY provider executable '{}': {}",
                        output.display(),
                        error
                    ))
                },
            )?;
        }
        found = true;
    }
    let candidate = dest.join(binary);
    if !found || !is_executable(&candidate) {
        return Err(OpyProviderError::install(
            "OPY provider archive does not contain an executable provider",
        ));
    }
    Ok(candidate)
}
