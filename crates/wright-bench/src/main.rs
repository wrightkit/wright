//! `wright-bench` — reproducible performance and resource benchmarks (#53).
//!
//! Measures compile latency, peak RSS, and generated-resource usage (emitted
//! Workshop bytes, WIR node counts) for the versioned corpus through the
//! real driver path (`CompilerSession::compile`), and enforces declared
//! regression thresholds. Output is versioned machine-readable JSON:
//! `target/wright-bench-report.json`. Exits non-zero when a threshold is
//! exceeded.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use wright_driver::CompilerSession;
use wright_driver::config::{SessionConfig, SourceKind};
use wright_transform::Profile;

/// The bench contract version (report consumers depend on it).
const BENCH_CONTRACT: &str = "wright-bench/v1";

/// Benchmark configuration with declared regression thresholds.
const BENCH_CONFIG: &str = r#"{
  "iterations": 5,
  "warmup": 1,
  "thresholds": {
    "maxMeanLatencyMs": 500.0,
    "maxEmittedBytes": 200000,
    "maxRssMb": 1024.0
  }
}"#;

fn main() -> ExitCode {
    match run() {
        Ok(ok) => {
            if ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(message) => {
            eprintln!("wright-bench: {message}");
            ExitCode::from(2)
        }
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn corpus_cases() -> Vec<(&'static str, PathBuf)> {
    [
        "synthetic/basic-rule",
        "synthetic/control-flow",
        "synthetic/declarations-rules",
        "synthetic/expressions-values",
        "synthetic/preprocessing",
        "real-world/overpy-cake",
    ]
    .into_iter()
    .map(|id| {
        (
            id,
            workspace_root()
                .join("compatibility/fixtures")
                .join(id)
                .join("source.opy"),
        )
    })
    .collect()
}

#[derive(serde::Serialize)]
struct FixtureReport {
    fixture: &'static str,
    mean_latency_ms: f64,
    min_latency_ms: f64,
    max_latency_ms: f64,
    emitted_bytes: usize,
    wir_values: usize,
    wir_actions: usize,
    wir_rules: usize,
}

#[derive(serde::Serialize)]
struct Report {
    contract: &'static str,
    version: &'static str,
    profile: &'static str,
    iterations: usize,
    thresholds: serde_json::Value,
    fixtures: Vec<FixtureReport>,
    summary: serde_json::Value,
}

fn run() -> Result<bool, String> {
    let config: serde_json::Value = serde_json::from_str(BENCH_CONFIG)
        .map_err(|error| format!("invalid bench config: {error}"))?;
    let iterations = config["iterations"].as_u64().unwrap_or(5) as usize;
    let warmup = config["warmup"].as_u64().unwrap_or(1) as usize;

    let mut reports = Vec::new();
    let mut regressions = Vec::new();

    for (id, path) in corpus_cases() {
        let source = std::fs::read_to_string(&path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let root = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();

        // Warmup (also populates caches deterministically).
        for _ in 0..warmup {
            compile(&source, id, &root)?;
        }

        let mut latencies = Vec::new();
        let mut last_output = None;
        let mut last_nodes = (0, 0, 0);
        for _ in 0..iterations {
            let start = Instant::now();
            let (output, nodes) = compile(&source, id, &root)?;
            latencies.push(start.elapsed());
            last_output = Some(output);
            last_nodes = nodes;
        }

        let emitted_bytes = last_output.map_or(0, |text| text.len());
        let (wir_values, wir_actions, wir_rules) = last_nodes;
        let mean_ms = mean(&latencies) * 1000.0;
        let report = FixtureReport {
            fixture: id,
            mean_latency_ms: mean_ms,
            min_latency_ms: min(&latencies) * 1000.0,
            max_latency_ms: max(&latencies) * 1000.0,
            emitted_bytes,
            wir_values,
            wir_actions,
            wir_rules,
        };
        if mean_ms
            > config["thresholds"]["maxMeanLatencyMs"]
                .as_f64()
                .unwrap_or(f64::MAX)
        {
            regressions.push(format!("{id}: latency {mean_ms:.1}ms exceeds threshold"));
        }
        if emitted_bytes as f64
            > config["thresholds"]["maxEmittedBytes"]
                .as_f64()
                .unwrap_or(f64::MAX)
        {
            regressions.push(format!(
                "{id}: emitted {emitted_bytes} bytes exceeds threshold"
            ));
        }
        reports.push(report);
    }

    // Peak RSS across the whole run (a stable, coarse resource bound).
    let rss_mb = peak_rss_mb();
    if rss_mb
        > config["thresholds"]["maxRssMb"]
            .as_f64()
            .unwrap_or(f64::MAX)
    {
        regressions.push(format!("peak RSS {rss_mb:.1} MB exceeds threshold"));
    }

    let report = Report {
        contract: BENCH_CONTRACT,
        version: env!("CARGO_PKG_VERSION"),
        profile: Profile::Compat.as_str(),
        iterations,
        thresholds: config["thresholds"].clone(),
        fixtures: reports,
        summary: serde_json::json!({
            "peakRssMb": rss_mb,
            "regressions": regressions,
        }),
    };
    let out = workspace_root().join("target");
    std::fs::create_dir_all(&out).map_err(|error| error.to_string())?;
    std::fs::write(
        out.join("wright-bench-report.json"),
        serde_json::to_string_pretty(&report).unwrap(),
    )
    .map_err(|error| error.to_string())?;
    println!("{}", serde_json::to_string_pretty(&report).unwrap());

    Ok(regressions.is_empty())
}

/// Compile one corpus source through the real driver path (compat profile)
/// and return the emitted text plus WIR node counts.
fn compile(
    source: &str,
    fixture: &str,
    root: &Path,
) -> Result<(String, (usize, usize, usize)), String> {
    let safe_name = fixture.replace('/', "-");
    let path =
        std::env::temp_dir().join(format!("wright-bench-{}-{safe_name}", std::process::id()));
    std::fs::write(&path, source).map_err(|error| error.to_string())?;
    let mut session = CompilerSession::new(SessionConfig {
        input: wright_driver::config::InputSpec::Path(path.clone()),
        kind: SourceKind::Opy,
        root: Some(root.to_path_buf()),
        profile: Profile::Compat,
        ..SessionConfig::default()
    })
    .map_err(|error| error.message)?;
    let envelope = session.compile();
    if !envelope.ok {
        return Err(format!(
            "{fixture}: compile failed: {:?}",
            envelope.diagnostics
        ));
    }
    let output = envelope.result.output.expect("compiled output");
    let loaded = session.load().map_err(|error| error.message)?;
    let nodes = (
        loaded.program.values.len(),
        loaded.program.actions.len(),
        loaded.program.rules.len(),
    );
    let _ = std::fs::remove_file(&path);
    Ok((output.text, nodes))
}

fn mean(latencies: &[Duration]) -> f64 {
    latencies.iter().map(|d| d.as_secs_f64()).sum::<f64>() / latencies.len() as f64
}

fn min(latencies: &[Duration]) -> f64 {
    latencies
        .iter()
        .map(|d| d.as_secs_f64())
        .fold(f64::MAX, f64::min)
}

fn max(latencies: &[Duration]) -> f64 {
    latencies
        .iter()
        .map(|d| d.as_secs_f64())
        .fold(0.0, f64::max)
}

/// Peak resident set size of this process, in MB.
/// `ru_maxrss` is bytes on macOS and KiB on Linux.
fn peak_rss_mb() -> f64 {
    unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        if libc::getrusage(libc::RUSAGE_SELF, &mut usage) == 0 {
            let rss = usage.ru_maxrss as f64;
            if cfg!(target_os = "macos") {
                rss / 1024.0 / 1024.0
            } else {
                rss / 1024.0
            }
        } else {
            0.0
        }
    }
}
