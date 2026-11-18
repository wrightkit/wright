//! Embeds the release version, which is the git tag: `v1.2.3` on a tagged
//! commit becomes `1.2.3`, and later commits become `1.2.3-N-gSHA`. A build
//! without git history reports the package version.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())
        .flatten()
        .map(|text| text.trim().to_owned())
}

fn main() {
    let version = git(&["describe", "--tags", "--match", "v[0-9]*", "--dirty"])
        .map(|tag| tag.trim_start_matches('v').to_owned())
        .unwrap_or_else(|| {
            std::env::var("CARGO_PKG_VERSION").expect("cargo sets the package version")
        });
    println!("cargo:rustc-env=WRIGHT_VERSION={version}");
    for path in ["HEAD", "packed-refs", "refs/tags"] {
        if let Some(path) = git(&["rev-parse", "--git-path", path]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}
