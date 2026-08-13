//! Language-service latency and memory evidence (#64/#68): representative
//! workflows (analyze, diagnostics, hover) over the heaviest corpus fixture,
//! with a machine-readable report and a generous regression bound. This is
//! the measured justification for the synchronous-recomputation cancellation
//! decision: each workflow is bounded well below interactive latency.

use std::path::PathBuf;
use std::time::Instant;

use wright_language::LanguageService;
use wright_language::document::{Document, Position};

const ITERATIONS: usize = 20;
/// A generous bound: each single-document workflow must finish well under
/// interactive latency (the corpus compiles in ~1–3 ms in the M8 benchmark).
const MAX_MEAN_MILLIS: f64 = 200.0;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn heaviest_document() -> Document {
    let root = workspace_root();
    let text = std::fs::read_to_string(
        root.join("compatibility/fixtures/real-world/overpy-cake/source.opy"),
    )
    .unwrap();
    Document::new("file:///cake.opy", text, root)
}

fn measure_mean(iterations: usize, mut f: impl FnMut()) -> f64 {
    let start = Instant::now();
    for _ in 0..iterations {
        f();
    }
    start.elapsed().as_secs_f64() * 1000.0 / iterations as f64
}

#[test]
fn language_service_workflows_are_interactively_bounded() {
    let document = heaviest_document();
    let mut service = LanguageService::new(document.root.clone());
    let uri = document.uri.clone();
    service.store.open(document);

    let analyze_ms = measure_mean(ITERATIONS, || {
        let _ = service.analyze(service.store.document(&uri).unwrap());
    });
    let diagnostics_ms = measure_mean(ITERATIONS, || {
        let _ = service.diagnostics(&uri);
    });
    let hover_ms = measure_mean(ITERATIONS, || {
        let _ = service.hover(
            &uri,
            Position {
                line: 0,
                character: 1,
            },
        );
    });

    let report = serde_json::json!({
        "workload": "overpy-cake single-document language service",
        "iterations": ITERATIONS,
        "meanMillis": {
            "analyze": analyze_ms,
            "diagnostics": diagnostics_ms,
            "hover": hover_ms,
        },
        "peakRssMb": peak_rss_mb(),
    });
    let out = workspace_root().join("target");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(
        out.join("language-service-perf.json"),
        serde_json::to_string_pretty(&report).unwrap(),
    )
    .unwrap();

    assert!(
        analyze_ms <= MAX_MEAN_MILLIS
            && diagnostics_ms <= MAX_MEAN_MILLIS
            && hover_ms <= MAX_MEAN_MILLIS,
        "language-service workflows are interactively bounded: {report}"
    );
}

/// Peak resident set size of this process, in MB.
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
