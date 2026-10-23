use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use wright_driver::result::{Envelope, LintPackageResult, LintPackageRow, exit, version_info};
use wright_driver::{
    Diagnostic, LintPackageError, LintPackageSource, LintPackageStore, RemoteCheck, Stage,
    StoreEntry,
};

use crate::cli;
use crate::present::{self, Presentation};

const API_BASE_ENV: &str = "WRIGHT_LINT_PACKAGE_API_BASE";
const CODELOAD_BASE_ENV: &str = "WRIGHT_LINT_PACKAGE_CODELOAD_BASE";

fn store() -> LintPackageStore {
    let mut store = LintPackageStore::new(LintPackageStore::default_root());
    if let Some(base) = env_nonempty(API_BASE_ENV) {
        store = store.with_api_base(base);
    }
    if let Some(base) = env_nonempty(CODELOAD_BASE_ENV) {
        store = store.with_codeload_base(base);
    }
    store
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

type Built = Result<(LintPackageResult, Vec<(Diagnostic, u8)>), LintPackageError>;

pub(crate) fn run(args: &cli::PackageArgs) -> ExitCode {
    match &args.command {
        cli::LintPackageCommand::Search(args) => {
            run_report("package-search", &args.output, |store| search(store, args))
        }
        cli::LintPackageCommand::Install(args) => {
            run_report("package-install", &args.output, |store| {
                install(store, args)
            })
        }
        cli::LintPackageCommand::List(args) => run_report("package-list", args, list),
        cli::LintPackageCommand::Outdated(args) => run_report("package-outdated", args, outdated),
        cli::LintPackageCommand::Update(args) => {
            run_report("package-update", &args.output, |store| update(store, args))
        }
        cli::LintPackageCommand::Remove(args) => {
            run_report("package-remove", &args.output, |store| remove(store, args))
        }
    }
}

fn run_report(
    command: &'static str,
    output: &cli::PackageOutputArgs,
    build: impl FnOnce(&LintPackageStore) -> Built,
) -> ExitCode {
    let pres = Presentation::from_output(output.format, output.renderer, output.color)
        .with_all(output.all);
    let store = store();
    let started = Instant::now();
    let envelope = match build(&store) {
        Ok((result, failures)) => {
            let ok = failures.is_empty();
            Envelope {
                wright: version_info(),
                command: command.to_string(),
                ok,
                exit: failures.first().map_or(exit::SUCCESS, |(_, code)| *code),
                diagnostics: failures.into_iter().map(|(d, _)| d).collect(),
                selection: None,
                gated: !ok,
                result,
                comparison: None,
            }
        }
        Err(error) => error_envelope(command, &store, &error),
    };
    let code = envelope.exit;
    present::render(&envelope, pres, started.elapsed(), None, None);
    ExitCode::from(code)
}

fn error_envelope(
    command: &str,
    store: &LintPackageStore,
    error: &LintPackageError,
) -> Envelope<LintPackageResult> {
    Envelope {
        wright: version_info(),
        command: command.to_string(),
        ok: false,
        exit: error.exit_code(),
        diagnostics: vec![Diagnostic::error(
            error.code(),
            stage_of(error),
            error.to_string(),
        )],
        selection: None,
        gated: false,
        result: LintPackageResult {
            store: root(store),
            ..LintPackageResult::default()
        },
        comparison: None,
    }
}

fn root(store: &LintPackageStore) -> String {
    store.root().display().to_string()
}

fn stage_of(error: &LintPackageError) -> Stage {
    match error {
        LintPackageError::Source(_) | LintPackageError::NotFound(_) => Stage::Discovery,
        LintPackageError::Invalid { .. } => Stage::Validation,
        _ => Stage::Internal,
    }
}

fn ok(result: LintPackageResult) -> Built {
    Ok((result, Vec::new()))
}

fn search(store: &LintPackageStore, args: &cli::PackageSearchArgs) -> Built {
    let page = store.search(args.query.as_deref())?;
    ok(LintPackageResult {
        store: root(store),
        total: Some(page.total),
        incomplete: Some(page.incomplete),
        packages: page
            .hits
            .iter()
            .map(|hit| LintPackageRow {
                source: hit.source.slug(),
                status: "found".to_string(),
                description: hit.description.clone(),
                ..LintPackageRow::default()
            })
            .collect(),
        next: vec!["wright lint package install <owner/repo>".to_string()],
    })
}

fn install(store: &LintPackageStore, args: &cli::PackageInstallArgs) -> Built {
    let (source, reference) = parse_source(&args.source, args.reference.as_deref())?;
    let package = store.install(&source, reference.as_deref())?;
    ok(LintPackageResult {
        store: root(store),
        packages: vec![installed_row(
            "installed",
            &source,
            package.sha,
            &package.dir,
            package.rules,
            package.downloaded,
        )],
        next: vec![
            "wright lint package list".to_string(),
            "wright lint package outdated".to_string(),
        ],
        ..LintPackageResult::default()
    })
}

fn list(store: &LintPackageStore) -> Built {
    let mut packages = Vec::new();
    for entry in store.entries()? {
        packages.push(match &entry.damage {
            Some(damage) => LintPackageRow {
                source: entry.source.slug(),
                status: "damaged".to_string(),
                sha: Some(entry.sha.clone()),
                dir: Some(dir_string(&entry.dir)),
                code: Some(damage.code().to_string()),
                message: Some(damage.to_string()),
                ..LintPackageRow::default()
            },
            None => installed_row(
                "installed",
                &entry.source,
                entry.sha,
                &entry.dir,
                entry.rules,
                false,
            ),
        });
    }
    ok(LintPackageResult {
        store: root(store),
        packages,
        next: vec!["wright lint package outdated".to_string()],
        ..LintPackageResult::default()
    })
}

fn outdated(store: &LintPackageStore) -> Built {
    let packages = store
        .entries()?
        .iter()
        .map(|entry| check_row(store, entry))
        .collect();
    ok(LintPackageResult {
        store: root(store),
        packages,
        next: vec!["wright lint package update".to_string()],
        ..LintPackageResult::default()
    })
}

fn update(store: &LintPackageStore, args: &cli::PackageUpdateArgs) -> Built {
    let entries = select_entries(store, args.source.as_deref())?;
    let mut packages = Vec::new();
    let mut failures = Vec::new();
    for entry in &entries {
        let row = update_row(store, entry);
        if let Some(failure) = row_failure(&row) {
            failures.push(failure);
        }
        packages.push(row);
    }
    Ok((
        LintPackageResult {
            store: root(store),
            packages,
            next: vec!["wright lint package list".to_string()],
            ..LintPackageResult::default()
        },
        failures,
    ))
}

fn remove(store: &LintPackageStore, args: &cli::PackageRemoveArgs) -> Built {
    let entries = select_entries(store, Some(args.source.as_str()))?;
    let mut packages = Vec::new();
    let mut failures = Vec::new();
    for entry in &entries {
        let row = match store.remove(&entry.source, &entry.sha) {
            Ok(removed) => LintPackageRow {
                source: entry.source.slug(),
                status: if removed.dir_removed {
                    "removed"
                } else {
                    "pruned"
                }
                .to_string(),
                sha: Some(entry.sha.clone()),
                dir: Some(dir_string(&entry.dir)),
                ..LintPackageRow::default()
            },
            Err(error) => LintPackageRow {
                source: entry.source.slug(),
                status: "error".to_string(),
                sha: Some(entry.sha.clone()),
                dir: Some(dir_string(&entry.dir)),
                code: Some(error.code().to_string()),
                message: Some(error.to_string()),
                ..LintPackageRow::default()
            },
        };
        if let Some(failure) = row_failure(&row) {
            failures.push(failure);
        }
        packages.push(row);
    }
    Ok((
        LintPackageResult {
            store: root(store),
            packages,
            next: vec!["wright lint package list".to_string()],
            ..LintPackageResult::default()
        },
        failures,
    ))
}

fn installed_row(
    status: &str,
    source: &LintPackageSource,
    sha: String,
    dir: &Path,
    rules: Vec<String>,
    downloaded: bool,
) -> LintPackageRow {
    LintPackageRow {
        source: source.slug(),
        status: status.to_string(),
        sha: Some(sha),
        dir: Some(dir_string(dir)),
        rules,
        downloaded: Some(downloaded),
        ..LintPackageRow::default()
    }
}

fn check_row(store: &LintPackageStore, entry: &StoreEntry) -> LintPackageRow {
    let mut row = LintPackageRow {
        source: entry.source.slug(),
        sha: Some(entry.sha.clone()),
        dir: Some(dir_string(&entry.dir)),
        ..LintPackageRow::default()
    };
    if let Some(damage) = &entry.damage {
        row.status = "error".to_string();
        row.code = Some(damage.code().to_string());
        row.message = Some(damage.to_string());
        return row;
    }
    match store.check_remote(&entry.source, None, &entry.sha) {
        RemoteCheck::Unchanged { sha } => {
            row.status = "unchanged".to_string();
            row.remote = Some(sha);
        }
        RemoteCheck::Updated { sha } => {
            row.status = "updated".to_string();
            row.remote = Some(sha);
        }
        RemoteCheck::Unavailable { reason } => {
            row.status = "unavailable".to_string();
            row.code = Some("lint-package-not-found".to_string());
            row.message = Some(reason);
        }
        RemoteCheck::Error(error) => {
            row.status = "error".to_string();
            row.code = Some(error.code().to_string());
            row.message = Some(error.to_string());
        }
    }
    row
}

fn update_row(store: &LintPackageStore, entry: &StoreEntry) -> LintPackageRow {
    let row = check_row(store, entry);
    if row.status != "updated" {
        return row;
    }
    let remote = row.remote.clone().expect("an updated row carries remote");
    match store.install(&entry.source, Some(&remote)) {
        Ok(package) => LintPackageRow {
            status: "updated".to_string(),
            dir: Some(dir_string(&package.dir)),
            rules: package.rules,
            downloaded: Some(package.downloaded),
            ..row
        },
        Err(error) => LintPackageRow {
            status: "error".to_string(),
            code: Some(error.code().to_string()),
            message: Some(error.to_string()),
            ..row
        },
    }
}

fn select_entries(
    store: &LintPackageStore,
    source: Option<&str>,
) -> Result<Vec<StoreEntry>, LintPackageError> {
    let entries = store.entries()?;
    let Some(input) = source else {
        return Ok(entries);
    };
    let (source, reference) = parse_source(input, None)?;
    let mut selected: Vec<StoreEntry> = entries
        .into_iter()
        .filter(|entry| entry.source == source)
        .collect();
    if let Some(reference) = &reference {
        let sha = store.resolve(&source, Some(reference))?;
        selected.retain(|entry| entry.sha == sha);
    }
    if selected.is_empty() {
        let mut message = format!("lint package '{source}' is not installed");
        if let Some(reference) = reference {
            message.push_str(&format!(" at ref '{reference}'"));
        }
        return Err(LintPackageError::NotFound(message));
    }
    Ok(selected)
}

fn row_failure(row: &LintPackageRow) -> Option<(Diagnostic, u8)> {
    if !matches!(row.status.as_str(), "unavailable" | "error") {
        return None;
    }
    let code = row.code.as_deref().unwrap_or("lint-package-remote");
    let message = format!(
        "{}: {}",
        row.source,
        row.message.as_deref().unwrap_or("operation failed")
    );
    let stage = match code {
        "lint-package-source" | "lint-package-not-found" => Stage::Discovery,
        "lint-package-invalid" => Stage::Validation,
        _ => Stage::Internal,
    };
    let exit = match code {
        "lint-package-source" => exit::USAGE,
        "lint-package-invalid" => exit::SOURCE_ERROR,
        _ => exit::INTERNAL,
    };
    Some((Diagnostic::error(code, stage, message), exit))
}

fn parse_source(
    input: &str,
    flag: Option<&str>,
) -> Result<(LintPackageSource, Option<String>), LintPackageError> {
    let (base, at_ref) = split_at_ref(input);
    let (source, url_ref) = LintPackageSource::parse(base)?;
    let mut refs: Vec<String> = Vec::new();
    for candidate in [
        url_ref,
        at_ref.map(str::to_string),
        flag.map(str::to_string),
    ] {
        if let Some(candidate) = candidate.filter(|r| !r.is_empty()) {
            refs.push(candidate);
        }
    }
    if let Some(conflict) = refs.iter().find(|r| **r != refs[0]) {
        return Err(LintPackageError::Source(format!(
            "conflicting refs for '{source}': '{}' and '{conflict}'",
            refs[0]
        )));
    }
    Ok((source, refs.into_iter().next()))
}

fn split_at_ref(input: &str) -> (&str, Option<&str>) {
    let Some((base, reference)) = input.rsplit_once('@') else {
        return (input, None);
    };
    if reference.is_empty() || base.matches('/').count() != 1 || base.contains(':') {
        return (input, None);
    }
    (base, Some(reference))
}

fn dir_string(dir: &Path) -> String {
    dir.display().to_string()
}
