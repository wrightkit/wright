//! Measures compile latency, peak RSS, and generated-resource usage.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use wright_driver::CompilerSession;
use wright_driver::Profile;
use wright_driver::config::{SessionConfig, SourceKind};

const BENCH_CONTRACT: &str = "wright-bench/v1";
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
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(m) => {
            eprintln!("wright-bench: {m}");
            ExitCode::from(2)
        }
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn benchmark_cases() -> Vec<(&'static str, PathBuf)> {
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
                .join("tests/fixtures/workshop")
                .join(id)
                .with_extension("ws"),
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
    let config: serde_json::Value =
        serde_json::from_str(BENCH_CONFIG).map_err(|e| format!("invalid bench config: {e}"))?;
    let iterations = config["iterations"].as_u64().unwrap_or(5) as usize;
    let warmup = config["warmup"].as_u64().unwrap_or(1) as usize;
    let root = workspace_root().join("tests/fixtures/workshop");

    let mut reports = Vec::new();
    let mut regressions = Vec::new();

    for (id, path) in benchmark_cases() {
        let source = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read fixture '{id}': {e}"))?;
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
        summary: serde_json::json!({ "peakRssMb": rss_mb, "regressions": regressions }),
    };
    let out = workspace_root().join("target");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    std::fs::write(
        out.join("wright-bench-report.json"),
        serde_json::to_string_pretty(&report).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    Ok(regressions.is_empty())
}

fn compile(
    source: &str,
    fixture: &str,
    root: &Path,
) -> Result<(String, (usize, usize, usize)), String> {
    let safe_name = fixture.replace('/', "-");
    let input_dir = workspace_root().join("target/wright-bench-inputs");
    std::fs::create_dir_all(&input_dir).map_err(|e| e.to_string())?;
    let path = input_dir.join(format!(
        "wright-bench-{}-{safe_name}.ws",
        std::process::id()
    ));
    std::fs::write(&path, source).map_err(|e| e.to_string())?;
    let mut session = CompilerSession::new(SessionConfig {
        input: wright_driver::config::InputSpec::Path(path.clone()),
        kind: SourceKind::Workshop,
        root: Some(root.to_path_buf()),
        profile: Profile::Compat,
        ..SessionConfig::default()
    })
    .map_err(|e| e.message)?;
    let envelope = session.compile();
    if !envelope.ok {
        return Err(format!(
            "{fixture}: compile failed: {:?}",
            envelope.diagnostics
        ));
    }
    let output = envelope.result.output.expect("compiled output");
    let loaded = session.load().map_err(|e| e.message)?;
    let nodes = (
        loaded
            .program
            .rules
            .iter()
            .flat_map(|r| {
                r.conditions
                    .iter()
                    .map(|c| count_value(&c.value))
                    .chain(r.actions.iter().map(count_action))
            })
            .sum(),
        loaded.program.rules.iter().map(|r| r.actions.len()).sum(),
        loaded.program.rules.len(),
    );
    let _ = std::fs::remove_file(&path);
    Ok((output.text, nodes))
}

fn count_action(action: &workshop_rs::Action) -> usize {
    use workshop_rs::Action::*;
    1 + match action {
        SetGlobalVariable { value, .. }
        | ModifyGlobalVariable { value, .. }
        | If { condition: value }
        | ElseIf { condition: value }
        | While { condition: value } => count_value(value),
        SetPlayerVariable { player, value, .. }
        | ModifyPlayerVariable { player, value, .. }
        | AssignMember {
            target: player,
            value,
            ..
        } => count_value(player) + count_value(value),
        ForGlobalVariable {
            start, stop, step, ..
        } => count_value(start) + count_value(stop) + count_value(step),
        ForPlayerVariable {
            start, stop, step, ..
        } => count_value(start) + count_value(stop) + count_value(step),
        Disabled { action } => count_action(action),
        Call { args, .. } => args.iter().map(count_value).sum(),
        CallSubroutine { .. } | Else | End => 0,
    }
}

fn count_value(value: &workshop_rs::Value) -> usize {
    use workshop_rs::Value::*;
    match value {
        Array(elements) => 1 + elements.iter().map(count_value).sum::<usize>(),
        Vector { x, y, z } => 1 + count_value(x) + count_value(y) + count_value(z),
        PlayerVariable { player, .. } => 1 + count_value(player),
        Call { args, .. } => 1 + args.iter().map(count_value).sum::<usize>(),
        _ => 1,
    }
}

fn mean(d: &[Duration]) -> f64 {
    d.iter().map(|t| t.as_secs_f64()).sum::<f64>() / d.len() as f64
}
fn min(d: &[Duration]) -> f64 {
    d.iter().map(|t| t.as_secs_f64()).fold(f64::MAX, f64::min)
}
fn max(d: &[Duration]) -> f64 {
    d.iter().map(|t| t.as_secs_f64()).fold(0.0, f64::max)
}

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

#[cfg(test)]
mod tests {
    use super::count_action;
    use workshop_rs::{Action, Value};

    #[test]
    fn benchmark_action_counts_preserve_v1_semantics() {
        assert_eq!(
            count_action(&Action::CallSubroutine {
                subroutine: "sub".to_string(),
            }),
            1
        );
        assert_eq!(count_action(&Action::Else), 1);
        assert_eq!(count_action(&Action::End), 1);
        assert_eq!(
            count_action(&Action::disabled(Action::CallSubroutine {
                subroutine: "sub".to_string(),
            })),
            2
        );
        assert_eq!(
            count_action(&Action::ForPlayerVariable {
                player: Value::Number(99.0),
                variable: "A".to_string(),
                start: Value::Number(1.0),
                stop: Value::Number(2.0),
                step: Value::Number(1.0),
            }),
            4
        );
    }
}
