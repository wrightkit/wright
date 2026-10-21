use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn wright() -> &'static str {
    env!("CARGO_BIN_EXE_wright")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn corpus_workshop(fixture_id: &str) -> String {
    std::fs::read_to_string(
        workspace_root()
            .join("tests/fixtures/workshop")
            .join(fixture_id)
            .with_extension("ws"),
    )
    .unwrap()
}

fn fixture_dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wright-gha-test-{}", name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (file, contents) in files {
        std::fs::write(dir.join(file), contents).unwrap();
    }
    dir
}

fn run_in(dir: &Path, args: &[&str], summary: Option<&Path>) -> Output {
    let mut command = Command::new(wright());
    command
        .args(args)
        .arg("--renderer")
        .arg("github-actions")
        .current_dir(dir)
        .stdin(Stdio::null())
        .env("GITHUB_ACTIONS", "true")
        .env_remove("CI")
        .env_remove("GITHUB_STEP_SUMMARY")
        .env_remove("GITHUB_SERVER_URL")
        .env_remove("GITHUB_REPOSITORY")
        .env_remove("GITHUB_SHA")
        .env_remove("NO_COLOR")
        .env_remove("FORCE_COLOR");
    if let Some(summary) = summary {
        command.env("GITHUB_STEP_SUMMARY", summary);
    }
    command.output().expect("wright runs")
}

fn transcript(output: &Output) -> String {
    format!(
        "exit {}\n== stdout\n{}\n== stderr\n{}",
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

const MIXED_CHECK: &str = "variables {\n\
     \tglobal:\n\
     \t\t0: B\n\
     }\n\
     \n\
     rule (\"r\") {\n\
     \tevent {\n\
     \t\tOngoing - Global;\n\
     \t}\n\
     \tactions {\n\
     \t\tSet Global Variable(B, FutureValueThing(2));\n\
     \t\tWhile(True);\n\
     \t\t\tSet Global Variable(B, 1);\n\
     \t\tEnd;\n\
     \t}\n\
     }\n";

const HOT_LOOP: &str = "rule (\"r\") {\n\
     \tevent {\n\
     \t\tOngoing - Global;\n\
     \t}\n\
     \tactions {\n\
     \t\tWhile(True);\n\
     \t\t\tWait(0.016, Ignore Condition);\n\
     \t\tEnd;\n\
     \t}\n\
     }\n";

#[test]
fn check_renders_annotation_group_and_summary() {
    let dir = fixture_dir("check", &[("mixed.ws", MIXED_CHECK)]);
    let summary = dir.join("summary.md");
    let output = run_in(&dir, &["check", "mixed.ws"], Some(&summary));
    insta::assert_snapshot!(transcript(&output));
    insta::assert_snapshot!(std::fs::read_to_string(&summary).unwrap());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lint_renders_warning_annotation_and_summary() {
    let dir = fixture_dir(
        "lint",
        &[("flow.ws", &corpus_workshop("synthetic/control-flow"))],
    );
    let summary = dir.join("summary.md");
    let output = run_in(&dir, &["lint", "flow.ws"], Some(&summary));
    insta::assert_snapshot!(transcript(&output));
    insta::assert_snapshot!(std::fs::read_to_string(&summary).unwrap());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn analyze_renders_risk_annotations_and_summary() {
    let dir = fixture_dir(
        "analyze",
        &[("cake.ws", &corpus_workshop("real-world/overpy-cake"))],
    );
    let summary = dir.join("summary.md");
    let output = run_in(&dir, &["analyze", "cake.ws"], Some(&summary));
    insta::assert_snapshot!(transcript(&output));
    insta::assert_snapshot!(std::fs::read_to_string(&summary).unwrap());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn annotations_stay_inside_the_per_step_limit() {
    let mut source = String::new();
    for index in 0..12 {
        source.push_str(&HOT_LOOP.replace("\"r\"", &format!("\"r{index}\"")));
    }
    let dir = fixture_dir("bounded", &[("loops.ws", &source)]);
    let output = run_in(&dir, &["lint", "loops.ws"], None);
    assert_eq!(output.status.code(), Some(0), "{}", transcript(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.matches("::warning ").count(), 10, "{stderr}");
    assert!(
        stderr.contains("2 annotation(s) withheld"),
        "the closing notice discloses the remainder: {stderr}"
    );
    assert_eq!(
        stderr.matches("finding: warning min-wait-loop").count(),
        12,
        "the group keeps the complete report: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn summary_links_to_the_blob_when_the_env_resolves() {
    let dir = fixture_dir("links", &[("mixed.ws", MIXED_CHECK)]);
    let summary = dir.join("summary.md");
    let mut command = Command::new(wright());
    command
        .args(["check", "mixed.ws", "--renderer", "github-actions"])
        .current_dir(&dir)
        .stdin(Stdio::null())
        .env("GITHUB_ACTIONS", "true")
        .env("GITHUB_STEP_SUMMARY", &summary)
        .env("GITHUB_SERVER_URL", "https://github.example")
        .env("GITHUB_REPOSITORY", "org/repo")
        .env("GITHUB_SHA", "abc123")
        .env_remove("CI")
        .env_remove("NO_COLOR")
        .env_remove("FORCE_COLOR");
    let output = command.output().expect("wright runs");
    assert_eq!(output.status.code(), Some(1), "{}", transcript(&output));
    let markdown = std::fs::read_to_string(&summary).unwrap();
    assert!(
        markdown.contains(
            "[`mixed.ws:11:26`](https://github.example/org/repo/blob/abc123/mixed.ws#L11)"
        ),
        "the leading item links to the blob: {markdown}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn summary_reports_a_produced_baseline_comparison() {
    let dir = fixture_dir(
        "baseline",
        &[("cake.ws", &corpus_workshop("real-world/overpy-cake"))],
    );
    let save = run_in(
        &dir,
        &["inspect", "cost", "cake.ws", "--save-baseline", "v1"],
        None,
    );
    assert_eq!(save.status.code(), Some(0), "{}", transcript(&save));
    let summary = dir.join("summary.md");
    let output = run_in(
        &dir,
        &["inspect", "cost", "cake.ws", "--baseline", "v1"],
        Some(&summary),
    );
    assert_eq!(output.status.code(), Some(0), "{}", transcript(&output));
    let markdown = std::fs::read_to_string(&summary).unwrap();
    assert!(
        markdown.contains("**Baseline baseline:v1**"),
        "the comparison reports in the summary: {markdown}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn summary_falls_back_to_a_notice_without_the_env() {
    let dir = fixture_dir("notice", &[("mixed.ws", MIXED_CHECK)]);
    let output = run_in(&dir, &["check", "mixed.ws"], None);
    assert_eq!(output.status.code(), Some(1), "{}", transcript(&output));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("::notice title=Wright summary::"),
        "the notice fallback carries the verdict: {stderr}"
    );
    assert!(stderr.contains("**ERROR** (exit 1)"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}
