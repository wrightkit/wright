//! The native side of the wasm parity check (#959): the same
//! `WrightCore` the WebAssembly module exports, driven over stdin/stdout
//! so the JavaScript test can feed both sides identical requests and
//! diff the response bodies.
//!
//!   wright-core-parity <source-file> <name>
//!
//! Reads one JSON envelope per stdin line and writes one response body
//! per stdout line, in order:
//!
//!   {"request": <body>}                              → core.request
//!   {"apply": {"transaction": …, "sources": {…}}}    → core.apply
//!
//! `<body>` may be a JSON object or a pre-serialized string. Run the
//! binary with cwd `/` so path-derived fields match the wasm build's
//! virtual root.

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: wright-core-parity <source-file> <name>");
        std::process::exit(2);
    }
    let text = match std::fs::read_to_string(&args[1]) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("cannot read '{}': {error}", args[1]);
            std::process::exit(2);
        }
    };
    let mut core = match wright_wasm::WrightCore::new(text, &args[2]) {
        Ok(core) => core,
        Err(message) => {
            println!(
                "{}",
                serde_json::json!({ "error": { "code": "init-refused", "message": message } })
            );
            std::process::exit(0);
        }
    };
    let stdin = std::io::stdin();
    for line in stdin.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let envelope: serde_json::Value = match serde_json::from_str(&line) {
            Ok(envelope) => envelope,
            Err(error) => {
                println!(
                    "{}",
                    serde_json::json!({ "error": { "code": "malformed-request", "message": error.to_string() } })
                );
                continue;
            }
        };
        let body = |value: &serde_json::Value| match value {
            serde_json::Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        if let Some(request) = envelope.get("request") {
            println!("{}", core.request(&body(request)));
        } else if let Some(apply) = envelope.get("apply") {
            println!(
                "{}",
                core.apply(&body(&apply["transaction"]), &body(&apply["sources"]))
            );
        } else {
            println!(
                "{}",
                serde_json::json!({ "error": { "code": "malformed-request", "message": "the parity envelope needs 'request' or 'apply'" } })
            );
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {}
