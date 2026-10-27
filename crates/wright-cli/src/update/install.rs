use std::path::{Path, PathBuf};
use std::process::Command as Process;

use super::{Platform, UpdateError, fetch, fetch_text, truncate};

pub(super) fn install_version(
    client: &reqwest::blocking::Client,
    version: &str,
    platform: Platform,
    base_url: &str,
    install_dir: &Path,
) -> Result<(), UpdateError> {
    let archive_name = platform.archive_name(version);
    let archive_url = format!(
        "{}/releases/{version}/{archive_name}",
        base_url.trim_end_matches('/')
    );
    let checksum_url = format!("{archive_url}.sha256");

    println!("==> downloading {archive_url}");
    let archive = fetch(client, &archive_url)?;
    let checksum = fetch_text(client, &checksum_url)?;
    println!("==> verifying SHA-256 checksum");
    verify_checksum(&archive, &checksum, &archive_name)?;

    ensure_writable(install_dir, platform)?;
    let staging = staging_dir(install_dir)?;
    let result = (|| -> Result<(), UpdateError> {
        unpack(&archive, &staging, platform)?;
        let payload = validated_payload(&staging, version, platform)?;
        // The matched pair is swapped lsp-first: `wright-lsp` may be held
        // open by an editor on Windows, and failing there changes nothing.
        let lsp = platform.exe("wright-lsp");
        replace_binary(
            &payload.join(&lsp),
            &install_dir.join(&lsp),
            platform,
            false,
        )?;
        let wright = platform.exe("wright");
        replace_binary(
            &payload.join(&wright),
            &install_dir.join(&wright),
            platform,
            true,
        )?;
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result?;

    smoke_check(install_dir, version, platform)?;
    println!(
        "==> done: wright and wright-lsp {version} installed in {}",
        install_dir.display()
    );
    let _ = crate::completion::refresh_installed_completions();
    Ok(())
}

fn staging_dir(install_dir: &Path) -> Result<PathBuf, UpdateError> {
    wright_driver::store::reserve_staging_dir(install_dir, ".wright-update", "an update")
        .map_err(UpdateError::failed)
}

fn unpack(archive: &[u8], dest: &Path, platform: Platform) -> Result<(), UpdateError> {
    let extracted = if platform.windows {
        zip::ZipArchive::new(std::io::Cursor::new(archive))
            .and_then(|mut zip| zip.extract(dest))
            .map_err(|e| e.to_string())
    } else {
        let decoder = flate2::read::GzDecoder::new(archive);
        tar::Archive::new(decoder)
            .unpack(dest)
            .map_err(|e| e.to_string())
    };
    extracted
        .map_err(|e| UpdateError::failed(format!("failed to extract the release archive: {e}")))
}

fn validated_payload(
    staging: &Path,
    version: &str,
    platform: Platform,
) -> Result<PathBuf, UpdateError> {
    let payload = staging.join(format!("wright-{version}-{}", platform.target));
    let (wright, lsp) = (platform.exe("wright"), platform.exe("wright-lsp"));
    if !payload.join(&wright).is_file() || !payload.join(&lsp).is_file() {
        return Err(UpdateError::failed(format!(
            "unexpected archive layout: expected '{wright}' and '{lsp}' inside {}",
            payload.display()
        )));
    }
    Ok(payload)
}

pub(super) fn verify_checksum(
    archive: &[u8],
    checksum_file: &str,
    archive_name: &str,
) -> Result<(), UpdateError> {
    wright_driver::store::verify_sha256(archive, checksum_file).map_err(|error| match error {
        wright_driver::store::ChecksumError::Empty => {
            UpdateError::failed(format!("empty checksum file for {archive_name}"))
        }
        wright_driver::store::ChecksumError::Invalid(digest) => UpdateError::failed(format!(
            "invalid checksum file for {archive_name}: expected one SHA-256 hex digest (got '{}')",
            truncate(&digest)
        )),
        wright_driver::store::ChecksumError::Mismatch(published, actual) => {
            UpdateError::failed(format!(
                "checksum verification failed for {archive_name} (published {published}, got {actual}); the download may be corrupted or tampered with — nothing was changed"
            ))
        }
    })
}

fn ensure_writable(install_dir: &Path, platform: Platform) -> Result<(), UpdateError> {
    let probe = install_dir.join(format!(".wright-update-probe-{}", std::process::id()));
    std::fs::write(&probe, b"").map_err(|e| {
        UpdateError::failed(format!(
            "installation directory {} is not writable ({e}); fix permissions or reinstall with the official installer ({})",
            install_dir.display(),
            platform.installer()
        ))
    })?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

/// Swap `source` into `destination`. Windows refuses to overwrite the running
/// executable, so the running `wright.exe` is replaced through `self_replace`.
#[cfg_attr(not(windows), allow(unused_variables))]
fn replace_binary(
    source: &Path,
    destination: &Path,
    platform: Platform,
    running: bool,
) -> Result<(), UpdateError> {
    let failed = |error: std::io::Error| {
        UpdateError::failed(format!(
            "could not replace {}: {error}; close programs using it or reinstall with the official installer ({})",
            destination.display(),
            platform.installer()
        ))
    };
    #[cfg(windows)]
    if running {
        return self_replace::self_replace(source).map_err(failed);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(source)
            .map_err(|e| UpdateError::failed(format!("could not read {}: {e}", source.display())))?
            .permissions()
            .mode();
        std::fs::set_permissions(source, std::fs::Permissions::from_mode(mode | 0o111)).map_err(
            |e| {
                UpdateError::failed(format!(
                    "could not make {} executable: {e}",
                    source.display()
                ))
            },
        )?;
    }
    std::fs::rename(source, destination).map_err(failed)
}

fn smoke_check(install_dir: &Path, version: &str, platform: Platform) -> Result<(), UpdateError> {
    check_version(&install_dir.join(platform.exe("wright")), version, platform)?;
    check_version(
        &install_dir.join(platform.exe("wright-lsp")),
        version,
        platform,
    )
}

fn check_version(exe: &Path, version: &str, platform: Platform) -> Result<(), UpdateError> {
    let output = Process::new(exe)
        .arg("--version")
        .output()
        .map_err(|error| {
            UpdateError::failed(format!(
                "smoke check failed: could not run {}: {error}",
                exe.display()
            ))
        })?;
    let reported = String::from_utf8_lossy(&output.stdout);
    if !reported.contains(version) {
        return Err(UpdateError::failed(format!(
            "smoke check failed: `{} --version` did not report version {version} (got {:?}); reinstall with the official installer ({})",
            exe.display(),
            reported.trim(),
            platform.installer()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::detect_platform_for;

    #[test]
    fn checksum_verification_accepts_good_and_rejects_bad() {
        let bytes = b"the archive bytes";
        let name = "wright-9.9.9-x86_64-unknown-linux-gnu.tar.gz";
        let good = format!("{}  {name}\n", wright_driver::sha256_hex(bytes));
        verify_checksum(bytes, &good, name).unwrap();

        let bad = format!("{}  archive\n", wright_driver::sha256_hex(b"other bytes"));
        let error = verify_checksum(bytes, &bad, "archive").unwrap_err();
        assert!(error.message().contains("checksum verification failed"));

        let malformed = "not-a-hash  archive\n";
        let error = verify_checksum(bytes, malformed, "archive").unwrap_err();
        assert!(error.message().contains("invalid checksum"));

        let short = "deadbeef\n";
        assert!(verify_checksum(bytes, short, "archive").is_err());
    }

    #[test]
    fn archive_layout_is_validated_before_install() {
        for target in ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"] {
            let os = if target.contains("windows") {
                "windows"
            } else {
                "linux"
            };
            let platform = detect_platform_for(os, "x86_64").unwrap();
            let (wright, lsp) = (platform.exe("wright"), platform.exe("wright-lsp"));

            let staged = scratch_dir(&format!("layout-{os}"));
            let bytes = build_archive(platform, &[(&wright, b"x")]);
            unpack(&bytes, &staged, platform).unwrap();
            let error = validated_payload(&staged, "9.9.9", platform).unwrap_err();
            assert!(error.message().contains("unexpected archive layout"));
            let _ = std::fs::remove_dir_all(&staged);

            let staged = scratch_dir(&format!("layout-ok-{os}"));
            let bytes = build_archive(platform, &[(&wright, b"x"), (&lsp, b"x")]);
            unpack(&bytes, &staged, platform).unwrap();
            let payload = validated_payload(&staged, "9.9.9", platform).unwrap();
            assert!(payload.join(&wright).is_file());
            assert!(payload.join(&lsp).is_file());
            let _ = std::fs::remove_dir_all(&staged);
        }
    }

    #[test]
    fn corrupt_archives_fail_to_unpack() {
        for os in ["linux", "windows"] {
            let platform = detect_platform_for(os, "x86_64").unwrap();
            let staged = scratch_dir(&format!("corrupt-{os}"));
            let error = unpack(b"not an archive", &staged, platform).unwrap_err();
            assert!(error.message().contains("failed to extract"));
            let _ = std::fs::remove_dir_all(&staged);
        }
    }

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wright-update-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn build_archive(platform: Platform, files: &[(&str, &[u8])]) -> Vec<u8> {
        let dir = format!("wright-9.9.9-{}", platform.target);
        if platform.windows {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            for (name, content) in files {
                writer
                    .start_file(
                        format!("{dir}/{name}"),
                        zip::write::SimpleFileOptions::default(),
                    )
                    .unwrap();
                std::io::Write::write_all(&mut writer, content).unwrap();
            }
            return writer.finish().unwrap().into_inner();
        }
        let mut builder = tar::Builder::new(Vec::new());
        for (name, content) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, format!("{dir}/{name}"), &content[..])
                .unwrap();
        }
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut encoder, &builder.into_inner().unwrap()).unwrap();
        encoder.finish().unwrap()
    }
}
