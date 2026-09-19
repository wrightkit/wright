//! Black-box CLI end-to-end tests (#41): the actual `wright` executable is
//! exercised across commands, inputs, output modes, exit codes, diagnostics,
//! and stdout/stderr separation — the automation contract of the CLI.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn wright() -> &'static str {
    env!("CARGO_BIN_EXE_wright")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn corpus_str(id: &str, file: &str) -> String {
    std::fs::read_to_string(
        workspace_root()
            .join("compatibility/fixtures")
            .join(id)
            .join(file),
    )
    .unwrap()
}

fn corpus_workshop(id: &str) -> String {
    corpus_str(id, "workshop.ws")
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "wcli-dir-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn str(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Fixture(#[allow(dead_code)] TempDir, PathBuf);

impl Fixture {
    fn new(name: &str, content: &str) -> Self {
        let dir = TempDir::new();
        let path = dir.path().join(name);
        std::fs::write(&path, content).unwrap();
        Self(dir, path)
    }
    fn corpus(id: &str) -> Self {
        Self::new("workshop.ws", &corpus_workshop(id))
    }
    fn corpus_opy(id: &str) -> Self {
        Self::new("source.opy", &corpus_str(id, "source.opy"))
    }
    fn path(&self) -> &Path {
        &self.1
    }
    fn str(&self) -> &str {
        self.1.to_str().unwrap()
    }
}

fn run(args: &[&str]) -> std::process::Output {
    run_with_env(args, &[])
}

fn run_with_env(args: &[&str], variables: &[(&str, &str)]) -> std::process::Output {
    let mut command = Command::new(wright());
    command
        .args(args)
        .stdin(Stdio::null())
        .env_remove("CI")
        .env_remove("GITHUB_ACTIONS")
        .env_remove("GITHUB_STEP_SUMMARY")
        .env_remove("NO_COLOR")
        .env_remove("FORCE_COLOR");
    for (name, value) in variables {
        command.env(name, value);
    }
    command.output().expect("wright runs")
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run_in_tty(args: &[&str]) -> std::process::Output {
    #[cfg(target_os = "linux")]
    let command = std::iter::once(wright())
        .chain(args.iter().copied())
        .map(|arg| {
            if arg
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || "-_/.:".contains(ch))
            {
                arg.to_string()
            } else {
                format!("'{}'", arg.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let mut script = Command::new("script");
    #[cfg(target_os = "linux")]
    script.args(["-qefc", &command, "/dev/null"]);
    #[cfg(target_os = "macos")]
    script.args(["-q", "/dev/null", wright()]).args(args);
    script
        .env("TERM", "xterm")
        .output()
        .expect("script is available")
}

fn run_with_stdin(args: &[&str], stdin: &str) -> std::process::Output {
    let mut child = Command::new(wright())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("wright runs");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn parse_json(output: &[u8]) -> serde_json::Value {
    serde_json::from_slice(output).expect("stdout is one JSON envelope")
}

trait OutputExt {
    fn stdout_str(&self) -> String;
    fn stderr_str(&self) -> String;
    fn json(&self) -> serde_json::Value;
    fn code(&self) -> i32;
    fn ok(&self) -> bool {
        self.code() == 0
    }
    fn diag_code(&self) -> String {
        self.json()["diagnostics"][0]["code"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }
}

impl OutputExt for std::process::Output {
    fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).to_string()
    }
    fn stderr_str(&self) -> String {
        String::from_utf8_lossy(&self.stderr).to_string()
    }
    fn json(&self) -> serde_json::Value {
        parse_json(&self.stdout)
    }
    fn code(&self) -> i32 {
        self.status.code().unwrap_or(-1)
    }
}

fn assert_exit_diag(args: &[&str], code: i32, diag: &str) -> serde_json::Value {
    let out = run(args);
    assert_eq!(out.code(), code);
    assert_eq!(out.diag_code(), diag);
    out.json()
}

#[test]
fn compile_and_check_clean_inputs() {
    let file = Fixture::corpus("synthetic/basic-rule");
    let check_out = run(&["check", file.str()]);
    assert_eq!(check_out.code(), 0);
    assert!(check_out.stdout_str().contains("PASS check"));

    let out = run(&["compile", file.str()]);
    assert!(out.ok() && out.stderr.is_empty());
    let stdout = out.stdout_str();
    assert!(stdout.contains("Disable Inspector Recording") && stdout.contains("Ongoing - Global"));

    let emit = Fixture::new("emitted.txt", "");
    let text = run(&["compile", file.str(), "-o", emit.str()]);
    assert!(text.ok() && text.stdout.is_empty() && text.stderr.is_empty());

    let output = run(&["compile", file.str(), "-o", emit.str(), "-f", "json"]);
    assert!(output.ok() && output.stderr.is_empty());
    let env = output.json();
    assert!(env["ok"] == true && env["exit"] == 0 && env["command"] == "compile");
    assert_eq!(env["wright"]["contract"], "wright-result/v1");
    assert_eq!(env["result"]["output"]["written_to"], emit.str());
    assert!(
        std::fs::read_to_string(emit.path())
            .unwrap()
            .contains("Disable Inspector Recording")
    );
}

#[test]
fn terminal_renderer_uses_command_specific_hierarchy() {
    let file = Fixture::corpus("synthetic/control-flow");
    for (cmd, head, det) in [
        ("check", "PASS check", "diagnostic(s)"),
        ("lint", "WARN lint", "Lint findings"),
        ("analyze", "PASS analyze", "Program overview"),
        ("inspect", "PASS inspect", "Program structure"),
    ] {
        let out = run(&[
            cmd,
            file.str(),
            "--renderer",
            "terminal",
            "--color",
            "never",
        ]);
        assert!(out.status.code().is_some());
        let s = out.stdout_str();
        assert!(s.contains(head) && s.contains(det));
    }
}

#[test]
fn analyze_real_project_report_is_bounded_and_ranked() {
    let file = Fixture::new(
        "pixelart.txt",
        &corpus_workshop("real-world/overpy-pixelart"),
    );
    let out = run(&[
        "analyze",
        file.str(),
        "--renderer",
        "terminal",
        "--color",
        "never",
    ]);
    assert!(out.ok());
    let s = out.stdout_str();
    assert!(s.contains("Program overview") && s.contains("Control-flow summary"));
    assert!(
        s.contains("Top rules (heuristic ranking")
            && s.contains("State and coupling")
            && s.contains("[static]")
    );
    assert!(s.lines().count() <= 40);
}

#[test]
fn check_malformed_and_query_commands() {
    let file = Fixture::new(
        "broken.txt",
        "rule (\"x\") { event { Ongoing - Global; } actions { If(True); }",
    );
    let out = run(&["check", file.str(), "-f", "json"]);
    assert_eq!(out.code(), 1);
    assert!(out.stderr.is_empty());
    let env = out.json();
    assert_eq!(env["ok"], false);
    assert_eq!(env["exit"], 1);
    let diag = &env["diagnostics"][0];
    assert!(diag["code"].is_string() && diag["severity"] == "error" && diag["span"].is_object());

    let ctrl = Fixture::corpus("synthetic/control-flow");
    let chk = run(&["check", ctrl.str(), "-f", "json"]);
    assert_eq!(chk.code(), 0);
    let diags = chk.json()["diagnostics"].as_array().unwrap().clone();
    assert!(diags.iter().all(|d| d["code"] != "min-wait-loop"));

    let ana = run(&["analyze", ctrl.str(), "-f", "json"]);
    assert!(ana.ok());
    let env_a = ana.json();
    assert!(env_a["result"]["program"]["findings"].is_null());
    assert!(
        !env_a["result"]["facts"]["symbols"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        !env_a["result"]["facts"]["rules"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let decl = Fixture::corpus("synthetic/declarations-rules");
    let insp = run(&["inspect", decl.str(), "-f", "json"]);
    assert!(insp.ok());
    assert!(
        !insp.json()["result"]["rules"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        !insp.json()["result"]["symbols"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn lint_over_workshop_input_reports_findings_in_text_and_json() {
    let file = Fixture::corpus("synthetic/control-flow");
    let text = run(&["lint", file.str()]);
    assert!(text.ok());
    let stdout = text.stdout_str();
    assert!(
        stdout.contains("WARN lint")
            && stdout.contains("min-wait-loop")
            && stdout.contains("evidence:")
    );

    let json = run(&["lint", file.str(), "-f", "json"]);
    assert!(json.ok());
    let env = json.json();
    assert_eq!(env["command"], "lint");
    assert_eq!(env["ok"], true);
    assert_eq!(env["result"]["input_identity"].as_str().unwrap().len(), 64);
    let findings = env["result"]["findings"].as_array().unwrap();
    assert!(!findings.is_empty());
    assert!(
        findings
            .iter()
            .all(|f| f["evidence"].is_string() && f["span"]["path"].is_string())
    );
    let rules = env["result"]["rules"].as_array().unwrap();
    assert!(!rules.is_empty());
    assert_eq!(
        env["result"]["config"]["rules"].as_object().unwrap().len(),
        rules.len()
    );
}

#[test]
fn span_path_is_consistent_across_input_spellings() {
    let dir = TempDir::new();
    let sub = dir.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let file_path = sub.join("loop.txt");
    std::fs::write(&file_path, corpus_workshop("synthetic/control-flow")).unwrap();

    let path_of = |out: std::process::Output| {
        assert!(out.ok());
        out.json()["result"]["findings"][0]["span"]["path"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let run_in = |d: &Path, p: &str| {
        Command::new(wright())
            .args(["lint", p, "-f", "json"])
            .current_dir(d)
            .output()
            .unwrap()
    };
    let abs = path_of(run(&["lint", file_path.to_str().unwrap(), "-f", "json"]));
    assert_eq!(abs, "loop.txt");
    assert_eq!(path_of(run_in(&sub, "loop.txt")), abs);
    assert_eq!(path_of(run_in(dir.path(), "sub/loop.txt")), abs);
}

#[test]
fn lint_rule_flags_control_findings() {
    let file = Fixture::corpus("synthetic/control-flow");
    let out = run(&[
        "lint",
        file.str(),
        "--disable-rule",
        "min-wait-loop",
        "-f",
        "json",
    ]);
    assert!(out.ok());
    let env = out.json();
    let findings = env["result"]["findings"].as_array().unwrap();
    assert!(findings.iter().all(|f| f["code"] != "min-wait-loop"));
    let rules = env["result"]["rules"].as_array().unwrap();
    assert_eq!(
        rules.iter().find(|r| r["id"] == "min-wait-loop").unwrap()["enabled"],
        false
    );

    let out = run(&[
        "lint",
        file.str(),
        "--rule-severity",
        "expensive-loop-check:warn",
        "-f",
        "json",
    ]);
    assert!(out.ok());
    let exp_loop = out.json()["result"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "expensive-loop-check")
        .unwrap()
        .clone();
    assert_eq!(exp_loop["effectiveSeverity"], "warning");
}

#[test]
fn lint_flags_are_usage_errors_for_other_commands() {
    for args in [
        &["check", "--disable-rule", "min-wait-loop"][..],
        &["analyze", "--rule-severity", "min-wait-loop:info"],
    ] {
        let out = run(args);
        assert!(out.code() == 2 && out.stdout.is_empty());
    }
}

#[test]
fn stdin_and_unknown_flags_and_extensions() {
    let out = run_with_stdin(&["check", "-"], &corpus_workshop("synthetic/basic-rule"));
    assert_eq!(out.code(), 0);

    let protocol = r#"{"protocol":{"name":"wright/opy-hir","version":"1.1.0"}}"#;
    let out_proto = run_with_stdin(&["check", "-"], protocol);
    assert_eq!(out_proto.code(), 1);
    assert!(out_proto.stderr_str().contains("input-kind-unsupported"));

    let out_opy = run_with_stdin(
        &["compile", "-", "--kind", "opy", "-f", "json"],
        &corpus_str("synthetic/basic-rule", "source.opy"),
    );
    assert_eq!(out_opy.code(), 3);
    assert_eq!(out_opy.diag_code(), "source-provider-unsupported");

    let file = Fixture::new("mystery.data", "whatever");
    let env = assert_exit_diag(
        &["check", file.str(), "-f", "json"],
        1,
        "input-kind-unknown",
    );
    assert!(
        env["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("--kind")
    );

    let unk = run(&["check", "--frobnicate"]);
    assert_eq!(unk.code(), 2);
    assert!(unk.stdout.is_empty() && !unk.stderr.is_empty());
}

#[test]
fn json_output_is_deterministic_across_runs() {
    let file = Fixture::corpus("synthetic/control-flow");
    let (first, second) = (
        run(&["analyze", file.str(), "-f", "json"]),
        run(&["analyze", file.str(), "-f", "json"]),
    );
    assert_eq!(first.stdout, second.stdout);
    assert!(!first.stdout.is_empty());
}

#[test]
fn stdout_stderr_separation_holds_in_both_modes() {
    let file = Fixture::corpus("synthetic/basic-rule");
    let text = run(&["check", file.str()]);
    assert!(text.stderr.is_empty() && text.stdout_str().contains("PASS check"));

    let json = run(&["check", file.str(), "-f", "json"]);
    assert!(json.stderr.is_empty());
    json.json();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn tty_progress_stops_and_clears_before_final_render() {
    let path = workspace_root().join("compatibility/fixtures/synthetic/control-flow/source.opy");
    let output = run_in_tty(&[
        "analyze",
        path.to_str().unwrap(),
        "--kind",
        "opy",
        "--renderer",
        "terminal",
        "--color",
        "never",
    ]);
    assert!(output.ok());
    let mut transcript = output.stdout;
    transcript.extend_from_slice(&output.stderr);
    let transcript = String::from_utf8_lossy(&transcript);
    let final_render = transcript
        .find("PASS analyze")
        .expect("final analyze render present");
    let before = &transcript[..final_render];
    assert!(
        before.contains("Starting workflow")
            && before.contains("Resolving input")
            && before.contains("Parsing")
            && before.contains("Resolving semantics")
    );
    let cleared = before.rfind("\x1b[2K").expect("activity line cleared");
    assert!(cleared < final_render && !transcript.contains("working…PASS"));

    let lint = run_in_tty(&[
        "lint",
        path.to_str().unwrap(),
        "--kind",
        "opy",
        "--renderer",
        "terminal",
        "--color",
        "never",
    ]);
    assert!(lint.ok());
    let mut lint_transcript = lint.stdout;
    lint_transcript.extend_from_slice(&lint.stderr);
    let s = String::from_utf8_lossy(&lint_transcript);
    assert!(s.contains("Running lint rules") && s.contains(" rules…"));
}

#[test]
fn non_interactive_renderers_have_no_progress_artifacts() {
    let file = Fixture::corpus_opy("synthetic/control-flow");
    for r in ["plain", "github-actions"] {
        let out = run_with_env(
            &["analyze", file.str(), "--renderer", r, "--color", "always"],
            &[("GITHUB_ACTIONS", "true")],
        );
        let combined = format!("{}{}", out.stdout_str(), out.stderr_str());
        assert!(
            !combined.contains("Resolving input")
                && !combined.contains("Running lint rules")
                && !combined.contains('⠋')
        );
    }
    let json = run(&["analyze", file.str(), "--format", "json"]);
    assert!(json.ok() && json.stderr.is_empty() && !json.stdout_str().contains("Resolving input"));
}

#[test]
fn version_help_and_locale_contract_surfaces() {
    let file = Fixture::corpus("synthetic/basic-rule");
    assert_eq!(run(&["check", file.str(), "--locale", "en-US"]).code(), 0);

    let output = run(&["version"]);
    assert!(output.ok());
    let banner = output.stdout_str();
    assert!(
        banner.starts_with("wright ")
            && banner.contains(env!("CARGO_PKG_VERSION"))
            && banner.contains("wright-driver")
    );
    assert_eq!(run(&["--version"]).stdout_str(), banner);

    let help = run(&["--help"]).stdout_str();
    for item in "compile convert check analyze lint inspect update --check --version --kind --target --locale --root --profile --format --renderer --color --disable-rule --rule-severity EXIT CODES".split_whitespace() {
        assert!(help.contains(item));
    }

    for shell in ["bash", "zsh", "fish", "powershell", "pwsh"] {
        let output = run(&["completion", shell]);
        assert!(output.ok() && output.stderr.is_empty());
        let c = output.stdout_str();
        assert!(c.contains("compile") && c.contains("renderer") && c.contains("color"));
    }
}

#[test]
fn completion_command_workflows_and_refusals() {
    let no_args = run(&["completion"]);
    assert_eq!(no_args.code(), 2);
    let err = no_args.stderr_str();
    assert!(
        no_args.stdout.is_empty() && err.contains("specify a shell") && err.contains("install")
    );

    if !cfg!(windows) {
        let keys = "WRIGHT_SHELL SHELL ZSH_VERSION BASH_VERSION FISH_VERSION PSModulePath POWERSHELL_DISTRIBUTION_CHANNEL PSExecutionPolicyPreference ZDOTDIR";
        let envs: Vec<(&str, &str)> = keys.split_whitespace().map(|k| (k, "")).collect();
        let out = run_with_env(&["completion", "install"], &envs);
        assert_eq!(out.code(), 1);
        let err = out.stderr_str();
        assert!(
            err.contains("could not automatically detect your shell")
                && err.contains("wright completion install <bash|zsh|fish|powershell>")
        );
    }

    let dir = TempDir::new();
    let run_inst = |args: &[&str]| {
        run(&[
            &["completion", "install", "zsh", "--dir", dir.str()][..],
            args,
        ]
        .concat())
    };
    assert!(
        run_inst(&[])
            .stdout_str()
            .contains("installed zsh completion")
    );
    assert!(dir.path().join("_wright").is_file());
    assert!(run_inst(&[]).stdout_str().contains("is already up to date"));
    assert!(
        run_inst(&["--force"])
            .stdout_str()
            .contains("updated zsh completion")
    );

    let dry = run(&[
        "completion",
        "install",
        "bash",
        "--dir",
        dir.str(),
        "--dry-run",
    ]);
    assert!(dry.ok() && dry.stdout_str().contains("would install bash completion"));
    assert!(!dir.path().join("wright").exists());

    let dir2 = TempDir::new();
    let env = [
        ("WRIGHT_SHELL", "fish"),
        ("WRIGHT_COMPLETION_DIR", dir2.str()),
    ];
    let out_env = run_with_env(&["completion", "install"], &env);
    assert!(out_env.ok() && out_env.stdout_str().contains("installed fish completion"));
    assert!(dir2.path().join("wright.fish").is_file());

    let dir3 = TempDir::new();
    let out_all = run(&["completion", "install", "--all", "--dir", dir3.str()]);
    assert!(out_all.ok());
    let s = out_all.stdout_str();
    for sh in ["bash", "zsh", "fish", "powershell"] {
        assert!(s.contains(&format!("installed {sh} completion")));
    }
    for f in ["wright", "_wright", "wright.fish", "_wright.ps1"] {
        assert!(dir3.path().join(f).is_file());
    }
}

#[test]
fn renderers_and_github_actions_formatting() {
    let broken = Fixture::new("broken.txt", "rule (\"x\") { event { Ongoing - Global; }");
    let env = &[("GITHUB_ACTIONS", "true")];

    let gh = run_with_env(
        &["check", broken.str(), "--renderer", "github-actions"],
        env,
    );
    assert_eq!(gh.code(), 1);
    assert!(gh.stderr_str().contains("::error") && gh.stderr_str().contains("::group::"));

    let plain = run_with_env(&["check", broken.str(), "--renderer", "plain"], env);
    assert_eq!(plain.code(), 1);
    assert!(!plain.stderr_str().contains("::error") && !plain.stderr_str().contains("\x1b["));

    let color = run_with_env(
        &[
            "check",
            broken.str(),
            "--renderer",
            "terminal",
            "--color",
            "always",
        ],
        &[("GITHUB_ACTIONS", "true"), ("NO_COLOR", "1")],
    );
    assert_eq!(color.code(), 1);
    assert!(color.stderr_str().contains("\x1b["));

    let json = run_with_env(
        &[
            "check",
            broken.str(),
            "-f",
            "json",
            "--renderer",
            "github-actions",
            "--color",
            "always",
        ],
        env,
    );
    assert_eq!(json.code(), 1);
    assert!(json.stderr.is_empty() && json.json()["wright"]["contract"] == "wright-result/v1");

    let clean = Fixture::corpus("synthetic/basic-rule");
    let compile = run_with_env(
        &["compile", clean.str(), "--renderer", "github-actions"],
        env,
    );
    assert_eq!(compile.code(), 0);
    assert!(
        compile.stdout_str().contains("Disable Inspector Recording")
            && !compile.stdout_str().contains("::")
    );
    assert!(compile.stderr_str().contains("::group::"));

    let exp = run(&["compile", clean.str(), "-f", "json"]).json()["result"]["output"]["text"]
        .as_str()
        .unwrap()
        .as_bytes()
        .to_vec();
    for r in ["plain", "github-actions"] {
        let out = run_with_env(&["compile", clean.str(), "--renderer", r], env);
        assert!(out.ok() && out.stdout == exp);
    }

    let sum = Fixture::new("summary.md", "");
    let out_sum = run_with_env(
        &["check", broken.str(), "--renderer", "github-actions"],
        &[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_STEP_SUMMARY", sum.str()),
        ],
    );
    assert_eq!(out_sum.code(), 1);
    assert!(!std::fs::read_to_string(sum.path()).unwrap().is_empty());
}

#[test]
fn missing_provider_analyze_and_compile_fail_with_distinct_diagnostics() {
    let fix = Fixture::corpus_opy("synthetic/basic-rule");
    let miss = fix.path().parent().unwrap().join("missing-opy-provider");
    let miss_str = miss.to_str().unwrap();
    assert_exit_diag(
        &[
            "compile",
            fix.str(),
            "--opy-provider",
            miss_str,
            "-f",
            "json",
        ],
        4,
        "provider-missing",
    );
    let env = assert_exit_diag(
        &[
            "analyze",
            fix.str(),
            "--opy-provider",
            miss_str,
            "-f",
            "json",
        ],
        4,
        "provider-missing",
    );
    assert!(env["result"]["program"].is_null());
}

#[test]
fn convert_commands_verify_target_and_provider_contracts() {
    let file = Fixture::corpus("synthetic/basic-rule");
    let out = run(&["convert", "--target", "opy", file.str(), "-f", "json"]);
    assert_eq!(out.diag_code(), "capability-unavailable");
    let env = out.json();
    assert!(
        !env["ok"].as_bool().unwrap()
            && env["command"] == "convert"
            && env["result"]["target"] == "opy"
    );
    assert_eq!(env["wright"]["contract"], "wright-result/v1");

    let ostw = assert_exit_diag(
        &["convert", "--target", "ostw", file.str(), "-f", "json"],
        4,
        "source-provider-unavailable",
    );
    assert!(!ostw["ok"].as_bool().unwrap() && ostw["diagnostics"][0]["stage"] == "internal");

    let no_flag = run(&["convert", file.str()]);
    assert_eq!(no_flag.code(), 2);
    assert!(no_flag.stdout.is_empty() && no_flag.stderr_str().contains("--target"));
    assert_eq!(run(&["convert", "--target", "nope", file.str()]).code(), 2);
    assert_eq!(run(&["check", "--target", "opy", file.str()]).code(), 2);

    let opy_file = Fixture::corpus_opy("synthetic/basic-rule");
    let bad_in = assert_exit_diag(
        &["convert", "--target", "ostw", opy_file.str(), "-f", "json"],
        3,
        "source-provider-unsupported",
    );
    assert!(bad_in["result"]["text"].as_str().unwrap().is_empty());
}
