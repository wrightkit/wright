//! `wright-catalog-gen` — the reproducible Workshop catalog data pipeline.
//!
//! Validates and deterministically canonicalizes the catalog data file.
//!
//! Usage:
//! ```sh
//! wright-catalog-gen check [--file catalog.json]
//! wright-catalog-gen build [--file catalog.json]
//! ```
//!
//! * `check` validates the catalog (schema, duplicate ids, colliding or
//!   missing aliases, undeclared locales) without writing.
//! * `build` validates and rewrites the file in canonical deterministic form
//!   (sorted keys, stable formatting). Re-running is byte-idempotent.
//!
//! Updating localization data is a bounded data change: edit the JSON and
//! re-run `check`/`build`; no parser or emitter code changes.

use std::path::PathBuf;
use std::process::ExitCode;

use wright_workshop::catalog::{Catalog, canonicalize};

const DEFAULT_FILE: &str = "src/catalog/data/catalog.json";

fn usage() -> &'static str {
    "usage: wright-catalog-gen <check|build> [--file catalog.json]"
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let command = args.next();
    let mut file = PathBuf::from(DEFAULT_FILE);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--file" => match args.next() {
                Some(path) => file = PathBuf::from(path),
                None => {
                    eprintln!("wright-catalog-gen: missing value for --file");
                    return ExitCode::from(2);
                }
            },
            other => {
                eprintln!("wright-catalog-gen: unknown argument '{other}'");
                eprintln!("{}", usage());
                return ExitCode::from(2);
            }
        }
    }

    let content = match std::fs::read_to_string(&file) {
        Ok(content) => content,
        Err(error) => {
            eprintln!(
                "wright-catalog-gen: cannot read {}: {error}",
                file.display()
            );
            return ExitCode::from(2);
        }
    };

    match command.as_deref() {
        Some("check") => match Catalog::load(&content) {
            Ok(catalog) => {
                println!(
                    "OK {} entries, {} enum domains, {} locale(s)",
                    catalog.entry_count(),
                    catalog.enum_domains_count(),
                    catalog.locales().len(),
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("wright-catalog-gen: {error}");
                ExitCode::from(1)
            }
        },
        Some("build") => match canonicalize(&content) {
            Ok(output) => match std::fs::write(&file, output) {
                Ok(()) => {
                    println!("wrote {}", file.display());
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!(
                        "wright-catalog-gen: cannot write {}: {error}",
                        file.display()
                    );
                    ExitCode::from(2)
                }
            },
            Err(error) => {
                eprintln!("wright-catalog-gen: {error}");
                ExitCode::from(1)
            }
        },
        _ => {
            eprintln!("{}", usage());
            ExitCode::from(2)
        }
    }
}
