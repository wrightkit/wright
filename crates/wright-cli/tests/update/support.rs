#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[path = "server.rs"]
mod server;

pub(crate) use server::MockServer;

pub(crate) const RELEASE: &str = "9.9.9";
pub(crate) const TRIPLE: &str = "x86_64-unknown-linux-gnu";
pub(crate) const PROVIDER_OLD: &str = "1.0.0";
pub(crate) const PROVIDER_RELEASE: &str = "1.4.0";

pub(crate) struct Release {
    pub(crate) files: HashMap<String, Vec<u8>>,
}

pub(crate) fn release(version: &str) -> Release {
    let archive = build_archive(version);
    let name = format!("wright-{version}-{TRIPLE}.tar.gz");
    let mut files = HashMap::new();
    files.insert(
        "/latest/version".to_string(),
        format!("{version}\n").into_bytes(),
    );
    files.insert(format!("/releases/{version}/{name}"), archive.clone());
    files.insert(
        format!("/releases/{version}/{name}.sha256"),
        format!("{}  {name}\n", wright_driver::sha256_hex(&archive)).into_bytes(),
    );
    Release { files }
}

fn build_archive(version: &str) -> Vec<u8> {
    let dir = format!("wright-{version}-{TRIPLE}");
    let mut builder = tar::Builder::new(Vec::new());
    for (name, content) in [
        (
            "wright",
            format!("#!/bin/sh\necho \"fake wright {version}\"\n").into_bytes(),
        ),
        (
            "wright-lsp",
            format!("#!/bin/sh\necho \"fake wright-lsp {version}\"\n").into_bytes(),
        ),
    ] {
        let path = format!("{dir}/{name}");
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, path, &content[..])
            .unwrap();
    }
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut encoder, &builder.into_inner().unwrap()).unwrap();
    encoder.finish().unwrap()
}

pub(crate) fn provider_target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("windows", "x86_64") => Some("x86_64-pc-windows-msvc"),
        _ => None,
    }
}

pub(crate) fn provider_binary() -> &'static str {
    if provider_target() == Some("x86_64-pc-windows-msvc") {
        "opy-provider.exe"
    } else {
        "opy-provider"
    }
}

pub(crate) fn provider_store(dir: &Path) -> PathBuf {
    dir.join("provider-data").join("providers").join("opy")
}

pub(crate) fn seed_provider(dir: &Path, version: &str) {
    let target = provider_target().expect("provider tests need a supported host");
    let store = provider_store(dir);
    let executable = store.join(version).join(target).join(provider_binary());
    std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
    std::fs::write(&executable, provider_body(version)).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::write(store.join("active"), format!("{version}\n")).unwrap();
}

pub(crate) fn provider_body(version: &str) -> Vec<u8> {
    format!("#!/bin/sh\necho \"fake opy-provider {version}\"\n").into_bytes()
}

pub(crate) fn provider_release(version: &str, files: &mut HashMap<String, Vec<u8>>) {
    let target = provider_target().expect("provider tests need a supported host");
    let binary = provider_binary();
    let name = format!("opy-provider-{version}-{target}.tar.gz");
    let mut builder = tar::Builder::new(Vec::new());
    let body = provider_body(version);
    let mut header = tar::Header::new_gnu();
    header.set_size(body.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    builder.append_data(&mut header, binary, &body[..]).unwrap();
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut encoder, &builder.into_inner().unwrap()).unwrap();
    let archive = encoder.finish().unwrap();
    files.insert(
        "/opy-rs/latest/version".to_string(),
        format!("{version}\n").into_bytes(),
    );
    files.insert(
        format!("/opy-rs/releases/{version}/{name}"),
        archive.clone(),
    );
    files.insert(
        format!("/opy-rs/releases/{version}/{name}.sha256"),
        format!("{}  {name}\n", wright_driver::sha256_hex(&archive)).into_bytes(),
    );
}

pub(crate) fn provider_env(server: &MockServer) -> Vec<(&'static str, String)> {
    let mut env = server.env();
    env.push((
        "WRIGHT_OPY_PROVIDER_BASE_URL",
        format!("{}/opy-rs", server.base_url()),
    ));
    env
}

pub(crate) fn install_dir(tag: &str) -> PathBuf {
    install_dir_under(std::env::temp_dir(), tag)
}

pub(crate) fn install_dir_under(parent: PathBuf, tag: &str) -> PathBuf {
    let dir = parent.join(format!("wright-update-it-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let wright = env!("CARGO_BIN_EXE_wright");
    std::fs::copy(wright, dir.join("wright")).unwrap();
    std::fs::copy(wright, dir.join("wright-lsp")).unwrap();
    dir
}

pub(crate) fn homebrew_install_dir(tag: &str) -> PathBuf {
    install_dir_under(std::env::temp_dir().join("homebrew"), tag)
}

pub(crate) fn run_update(
    dir: &Path,
    args: &[&str],
    env: &[(&'static str, String)],
) -> std::process::Output {
    let mut command = Command::new(dir.join("wright"));
    command
        .args(args)
        .env("WRIGHT_INSTALL_OS", "linux")
        .env("WRIGHT_INSTALL_ARCH", "x86_64")
        .env("WRIGHT_PROVIDER_DATA_DIR", dir.join("provider-data"));
    for (key, value) in env {
        command.env(key, value);
    }
    command
        .stdin(Stdio::null())
        .output()
        .expect("wright update runs")
}

pub(crate) fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}
