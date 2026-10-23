use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use wright_driver::service::{ToolResponse, ToolService};
use wright_driver::{CompilerSession, InputSpec, SessionConfig, SourceKind};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn assert_default_surface_parity(operation: &str, args: &[&str], request: Value) {
    let fixture = if matches!(operation, "lint" | "analyze" | "costEstimate") {
        "control-flow"
    } else {
        "declarations-rules"
    };
    let input = workspace_root().join(format!("tests/fixtures/workshop/synthetic/{fixture}.ws"));
    assert_default_surface_parity_on_input(
        operation,
        args,
        request.clone(),
        input,
        SourceKind::Workshop,
        None,
    );
    let seed =
        workspace_root().join("benchmarks/metrics/corpus/seed-understand-score-flow/mode.ws");
    assert_default_surface_parity_on_input(
        operation,
        args,
        request.clone(),
        seed,
        SourceKind::Workshop,
        None,
    );
    let provider = std::env::var_os("WRIGHT_OPY_PROVIDER")
        .map(PathBuf::from)
        .or_else(|| {
            wright_driver::OpyProviderConfig::default()
                .installed()
                .expect("local provider inventory")
                .map(|p| p.executable)
        });
    if let Some(provider) = provider {
        let seed =
            workspace_root().join("benchmarks/metrics/corpus/seed-understand-opy-project/mode.opy");
        assert_default_surface_parity_on_input(
            operation,
            args,
            request,
            seed,
            SourceKind::Opy,
            Some(provider),
        );
    } else {
        eprintln!("SKIPPED OPY seed parity: no locally installed provider or WRIGHT_OPY_PROVIDER");
    }
}

fn assert_default_surface_parity_on_input(
    operation: &str,
    args: &[&str],
    mut request: Value,
    input: PathBuf,
    kind: SourceKind,
    provider: Option<PathBuf>,
) {
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(input.clone()),
        kind,
        source_backend: wright_driver::SourceBackend::Auto,
        opy_provider: provider
            .clone()
            .map(wright_driver::OpyProviderConfig::with_executable)
            .unwrap_or_default(),
        ..Default::default()
    })
    .unwrap();
    let mut target = None;
    if operation == "cfg" || operation == "references" {
        let values = if operation == "cfg" {
            session.rules(None, None, None, false).result.0
        } else {
            session.symbols(None, None, None).result.0
        };
        let items = values
            .as_array()
            .or_else(|| {
                values
                    .get(if operation == "cfg" {
                        "rules"
                    } else {
                        "symbols"
                    })
                    .and_then(Value::as_array)
            })
            .unwrap();
        target = items
            .iter()
            .filter(|item| {
                operation == "cfg"
                    || matches!(
                        item["kind"].as_str(),
                        Some("globalVariable" | "playerVariable")
                    )
            })
            .filter_map(|item| item["name"].as_str())
            .find(|name| {
                !name.is_empty()
                    && items
                        .iter()
                        .filter(|item| item["name"].as_str() == Some(*name))
                        .count()
                        == 1
            })
            .map(str::to_owned);
        let name = target
            .as_ref()
            .expect("seed has an unambiguous query target");
        request[if operation == "cfg" { "rule" } else { "symbol" }] = json!(name);
    }
    let mut service = ToolService::new(&mut session).unwrap();
    let response = service.handle(&serde_json::from_value(request).unwrap());
    let ToolResponse::Ok { mut result } = response else {
        panic!(
            "{operation} agent failed on {}: {response:?}",
            input.display()
        )
    };
    let run = |json: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_wright"));
        let mut selected_args = args.to_vec();
        if let Some(target) = &target {
            selected_args[2] = target;
        }
        command
            .args(selected_args)
            .arg(&input)
            .args([
                "--kind",
                if kind == SourceKind::Opy {
                    "opy"
                } else {
                    "workshop"
                },
                "--renderer",
                "plain",
                "--color",
                "never",
            ])
            .stdin(Stdio::null());
        if let Some(provider) = &provider {
            command.arg("--opy-provider").arg(provider);
        }
        if json {
            command.args(["--format", "json"]);
        }
        command.output().unwrap()
    };
    let output = run(true);
    let cli: Value = serde_json::from_slice(&output.stdout).unwrap();
    if operation == "lint" {
        result.as_object_mut().unwrap().remove("snapshot");
    }
    if result.get("result").is_some() && result.get("command").is_some() {
        assert_eq!(result["ok"], cli["ok"], "{operation} verdict");
        assert_eq!(result["exit"], cli["exit"], "{operation} exit");
        result = result["result"].clone();
    } else if result.get("ok").is_some() {
        let object = result
            .as_object_mut()
            .expect("an 'ok' key implies an object");
        if let Some(ok) = object.remove("ok") {
            assert_eq!(ok, cli["ok"], "{operation} verdict");
        }
        if let Some(exit) = object.remove("exit") {
            assert_eq!(exit, cli["exit"], "{operation} exit");
        }
    }
    assert_eq!(cli["result"], result, "{operation} structured items");
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains('\n')
            || output.stdout.iter().filter(|&&b| b == b'\n').count() == 1,
        "compact JSON"
    );
    assert_eq!(
        output.stdout,
        run(true).stdout,
        "{operation} byte determinism"
    );
    let text_output = run(false);
    assert_eq!(text_output.status.code(), output.status.code());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&text_output.stdout),
        String::from_utf8_lossy(&text_output.stderr)
    );
    let list = |key: &str| {
        result
            .as_array()
            .or_else(|| result.get(key).and_then(Value::as_array))
            .or_else(|| result.get("items").and_then(Value::as_array))
            .map_or(&[][..], Vec::as_slice)
    };
    let keys: &[&str] = match operation {
        "lint" | "costEstimate" => &["findings"],
        "analyze" => &["hotspots", "risks", "variables"],
        "inspect" | "rules" => &["rules"],
        "symbols" => &["symbols"],
        "references" => &["references"],
        "cfg" => &["blocks"],
        "callGraph" => &["edges"],
        _ => &[],
    };
    for key in keys {
        for item in list(key) {
            for field in ["name", "message", "caller", "callee"] {
                if let Some(value) = item[field].as_str().filter(|v| !v.is_empty()) {
                    assert!(
                        text.contains(value),
                        "{operation} text omitted {field}: {value}\n{text}"
                    );
                }
            }
            if *key == "references"
                && let (Some(line), Some(col)) = (
                    item["span"]["start"]["line"].as_u64(),
                    item["span"]["start"]["col"].as_u64(),
                )
            {
                assert!(
                    text.contains(&format!(":{line}:{col}")),
                    "reference location absent: {text}"
                );
            }
            if *key == "blocks"
                && let Some(id) = item["id"].as_u64()
            {
                assert!(
                    text.contains(&format!("block: id={id}")),
                    "cfg block absent: {text}"
                );
            }
        }
    }
}

macro_rules! default_surface_test {
    ($name:ident,$op:literal,$args:expr,$request:expr) => {
        #[test]
        fn $name() {
            assert_default_surface_parity($op, $args, $request);
        }
    };
}
default_surface_test!(
    check_default_surfaces_agree,
    "check",
    &["check"],
    json!({"op":"check"})
);
default_surface_test!(
    lint_default_surfaces_agree,
    "lint",
    &["lint"],
    json!({"op":"lint"})
);
default_surface_test!(
    analyze_default_surfaces_agree,
    "analyze",
    &["analyze"],
    json!({"op":"analyze"})
);
default_surface_test!(
    inspect_default_surfaces_agree,
    "inspect",
    &["inspect"],
    json!({"op":"inspect"})
);
default_surface_test!(
    rules_default_surfaces_agree,
    "rules",
    &["inspect", "rules"],
    json!({"op":"rules"})
);
default_surface_test!(
    symbols_default_surfaces_agree,
    "symbols",
    &["inspect", "symbols"],
    json!({"op":"symbols"})
);
default_surface_test!(
    references_default_surfaces_agree,
    "references",
    &["inspect", "refs", "score"],
    json!({"op":"references","symbol":"score"})
);
default_surface_test!(
    cfg_default_surfaces_agree,
    "cfg",
    &["inspect", "cfg", "player starts"],
    json!({"op":"cfg","rule":"player starts"})
);
default_surface_test!(
    callgraph_default_surfaces_agree,
    "callGraph",
    &["inspect", "callgraph"],
    json!({"op":"callGraph"})
);
default_surface_test!(
    cost_default_surfaces_agree,
    "costEstimate",
    &["inspect", "cost"],
    json!({"op":"costEstimate"})
);
