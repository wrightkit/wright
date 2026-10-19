use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// `wright ui` serves the page built under `ui/dist` (ADR-0032): with the
/// `ui` feature every file there is embedded into the binary so a release
/// archive is self-contained. Cargo always runs this script, so the asset
/// table is empty unless the feature is on — a Rust-only build never needs
/// Node. A debug build with the feature but no `ui/dist` embeds a stub
/// page instead of failing; a release build fails, so a packaged binary
/// can never ship the stub.
fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set")).join("ui_assets.rs");
    let mut source = String::from("pub(crate) static UI_ASSETS: &[(&str, &str, &[u8])] = &[\n");
    if env::var_os("CARGO_FEATURE_UI").is_none() {
        source.push_str("];\n");
        fs::write(&out, source).expect("write ui_assets.rs");
        return;
    }

    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    let mut files = Vec::new();
    if dist.is_dir() {
        collect(&dist, &mut files);
        files.sort();
    }
    if files.is_empty() {
        assert_ne!(
            env::var("PROFILE").as_deref(),
            Ok("release"),
            "the 'ui' feature embeds ui/dist, which is empty; run `npm ci && npm run build` in ui/ first"
        );
        println!(
            "cargo:warning=ui/dist is empty; `wright ui` serves a stub page until `npm run build` runs in ui/"
        );
        source.push_str(
            "(\"index.html\", \"text/html; charset=utf-8\", b\"<!doctype html><html lang=en><title>Wright</title><body><p>Wright UI assets were not embedded in this build; run <code>npm run build</code> in <code>ui/</code>.\"),\n",
        );
    }
    for file in files {
        let serve_path = file.strip_prefix(&dist).expect("under dist").to_path_buf();
        // Serve paths are URL paths: emit `/`-separated spellings, which
        // `include_bytes!` also accepts on every build platform.
        let serve_path = serve_path.to_string_lossy().replace('\\', "/");
        source.push_str(&format!(
            "(\"{serve_path}\", \"{}\", include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../ui/dist/{serve_path}\"))),\n",
            mime(&serve_path)
        ));
    }
    source.push_str("];\n");
    fs::write(&out, source).expect("write ui_assets.rs");
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("ui/dist is readable") {
        let path = entry.expect("ui/dist entry").path();
        if path.is_dir() {
            collect(&path, files);
        } else if path.is_file() {
            files.push(path);
        }
    }
}

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "woff2" => "font/woff2",
        "map" => "application/json",
        _ => "application/octet-stream",
    }
}
