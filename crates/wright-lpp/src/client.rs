//! Newline-delimited JSON-RPC 2.0 client over a byte transport.
//!
//! This module owns the wire mechanics of the LPP client: newline-delimited
//! framing (LF writes, LF/CRLF and empty-line-tolerant reads), correlation
//! ids, request/response matching, request timeouts, the LPP session phase
//! machine (`initialize` → ready → `shutdown`), and deterministic handling
//! of provider output that is not a valid LPP v1 response.
//!
//! The failure policy is strict and deterministic:
//!
//! * A provider message that **cannot be attributed to a pending request**
//!   (not valid JSON, a batch, a message without an integer id, wrong
//!   `jsonrpc` version, an id that matches no pending request) poisons the
//!   session: every pending request fails with a `Malformed` error and all
//!   subsequent requests fail with the same recorded violation.
//! * A provider message that **is attributable to a pending request** but
//!   whose payload is malformed (both `result` and `error`, neither, or an
//!   LPP-shaped error without `data.lpp`) fails that request with a
//!   `Malformed` error; the session stays usable.
//! * End-of-stream on the provider output fails every pending request with
//!   `Exited` and records the exit for subsequent requests.
//! * A response that does not arrive within [`ClientConfig::request_timeout`]
//!   fails the request with `Timeout`; a late response is ignored (never
//!   treated as a protocol violation).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::error::{LppError, LppErrorKind, ProviderError};

/// Client tuning.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// How long to wait for a response before failing the request with
    /// `ProviderError::Timeout`.
    pub request_timeout: Duration,
}

impl Default for ClientConfig {
    fn default() -> Self {
        ClientConfig {
            request_timeout: Duration::from_secs(30),
        }
    }
}

/// The session phase of the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientPhase {
    /// No successful `lpp/initialize` yet; only initialize is allowed.
    Fresh,
    /// Initialization succeeded; document-scoped requests are allowed.
    Ready,
    /// `lpp/shutdown` was sent; nothing further is allowed.
    ShutDown,
}

/// Reader-thread shared state: pending requests plus recorded session
/// failures.
#[derive(Default)]
struct Shared {
    pending: HashMap<i64, Sender<Result<Value, ProviderError>>>,
    /// The first protocol violation observed (session-poisoning).
    violation: Option<ProviderError>,
    /// The provider output closed (session-dead).
    exited: Option<ProviderError>,
}

/// A JSON-RPC 2.0 client with LPP v1 session semantics over a byte
/// transport (typically the stdio pipes of a spawned provider process).
pub struct JsonRpcClient {
    writer: Option<Box<dyn Write + Send>>,
    shared: Arc<Mutex<Shared>>,
    /// Kept alive for the reader thread; joining happens only when the
    /// transport closes.
    _reader: Option<JoinHandle<()>>,
    next_id: i64,
    phase: ClientPhase,
    timeout: Duration,
}

impl JsonRpcClient {
    /// Build a client over a reader/writer pair (for example a provider
    /// process's stdout/stdin).
    ///
    /// A reader thread consumes `reader` line by line and routes responses
    /// to their pending requests. The thread ends when the transport reaches
    /// end-of-stream.
    pub fn new(
        reader: Box<dyn Read + Send>,
        writer: Box<dyn Write + Send>,
        config: ClientConfig,
    ) -> JsonRpcClient {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let thread_shared = Arc::clone(&shared);
        let reader_thread = std::thread::Builder::new()
            .name("wright-lpp-reader".to_string())
            .spawn(move || reader_loop(reader, thread_shared))
            .expect("spawning the LPP reader thread cannot fail");
        JsonRpcClient {
            writer: Some(writer),
            shared,
            _reader: Some(reader_thread),
            next_id: 0,
            phase: ClientPhase::Fresh,
            timeout: config.request_timeout,
        }
    }

    /// The current session phase.
    pub fn phase(&self) -> ClientPhase {
        self.phase
    }

    /// The configured request timeout.
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Send `lpp/initialize` (the first message of a session).
    ///
    /// Only allowed in the fresh phase. On success the session becomes
    /// ready. On failure the session stays fresh so the caller can retry
    /// with a supported protocol version or terminate.
    pub fn initialize(&mut self, params: Value) -> Result<Value, ProviderError> {
        match self.phase {
            ClientPhase::Fresh => {}
            ClientPhase::Ready => return Err(ProviderError::AlreadyInitialized),
            ClientPhase::ShutDown => {
                return Err(ProviderError::ShutDown {
                    method: "lpp/initialize".to_string(),
                });
            }
        }
        let value = self.send("lpp/initialize", params)?;
        self.phase = ClientPhase::Ready;
        Ok(value)
    }

    /// Send a document-scoped request. Only allowed once the session is
    /// ready.
    pub fn request(&mut self, method: &str, params: Value) -> Result<Value, ProviderError> {
        match self.phase {
            ClientPhase::Fresh => {
                return Err(ProviderError::NotInitialized {
                    method: method.to_string(),
                });
            }
            ClientPhase::ShutDown => {
                return Err(ProviderError::ShutDown {
                    method: method.to_string(),
                });
            }
            ClientPhase::Ready => {}
        }
        self.send(method, params)
    }

    /// Send `lpp/shutdown` and await the null result. Only allowed once the
    /// session is ready; afterwards the session is shut down.
    pub fn shutdown(&mut self) -> Result<(), ProviderError> {
        match self.phase {
            ClientPhase::Fresh => {
                return Err(ProviderError::NotInitialized {
                    method: "lpp/shutdown".to_string(),
                });
            }
            ClientPhase::ShutDown => {
                return Err(ProviderError::ShutDown {
                    method: "lpp/shutdown".to_string(),
                });
            }
            ClientPhase::Ready => {}
        }
        let value = self.send("lpp/shutdown", json!({}))?;
        if !value.is_null() {
            return Err(ProviderError::Malformed {
                detail: "lpp/shutdown result must be null".to_string(),
            });
        }
        self.phase = ClientPhase::ShutDown;
        Ok(())
    }

    /// Close the writer (the provider's stdin reaches end-of-file, which LPP
    /// requires a provider to treat as a prompt clean exit).
    pub fn close_stdin(&mut self) {
        self.writer = None;
    }

    /// Return the session to the fresh phase after a failed initialize.
    ///
    /// The wire-level initialize only marks the session ready after a
    /// successful response; callers that then fail to validate the response
    /// (malformed result, protocol version echo mismatch) use this to keep
    /// the session restartable instead of wedged.
    pub fn reset_initialize(&mut self) {
        if matches!(self.phase, ClientPhase::Ready) {
            self.phase = ClientPhase::Fresh;
        }
    }

    /// Write a request, wait for its correlated response, and validate the
    /// session state.
    fn send(&mut self, method: &str, params: Value) -> Result<Value, ProviderError> {
        {
            let shared = self.shared.lock().expect("LPP reader state lock poisoned");
            if let Some(error) = &shared.violation {
                return Err(error.clone());
            }
            if let Some(error) = &shared.exited {
                return Err(error.clone());
            }
        }

        self.next_id += 1;
        let id = self.next_id;
        let (tx, rx) = mpsc::channel();
        {
            let mut shared = self.shared.lock().expect("LPP reader state lock poisoned");
            // Re-check under the lock: a violation may have been recorded
            // while waiting for it.
            if let Some(error) = &shared.violation {
                return Err(error.clone());
            }
            if let Some(error) = &shared.exited {
                return Err(error.clone());
            }
            shared.pending.insert(id, tx);
        }

        let line = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        })
        .to_string();
        let writer = self.writer.as_mut().ok_or_else(|| ProviderError::Io {
            message: format!("cannot send '{method}': the provider stdin is closed"),
        })?;
        if let Err(error) = write_line(writer, &line) {
            // Best effort: if the reader already recorded an exit, report
            // that; otherwise the write failure is an I/O failure.
            let shared = self.shared.lock().expect("LPP reader state lock poisoned");
            if let Some(exited) = &shared.exited {
                return Err(exited.clone());
            }
            return Err(ProviderError::Io {
                message: format!("cannot write request '{method}': {error}"),
            });
        }

        match rx.recv_timeout(self.timeout) {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(error)) => Err(error),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(ProviderError::Timeout {
                method: method.to_string(),
                duration: self.timeout,
            }),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // The reader thread ended without routing this request. The
                // session is dead; report the recorded state if any.
                let shared = self.shared.lock().expect("LPP reader state lock poisoned");
                if let Some(error) = &shared.violation {
                    return Err(error.clone());
                }
                if let Some(error) = &shared.exited {
                    return Err(error.clone());
                }
                Err(ProviderError::Exited {
                    status: None,
                    message: format!(
                        "the LPP provider connection ended while '{method}' was pending"
                    ),
                })
            }
        }
    }
}

/// Write one line of framing (LF terminator, flushed).
fn write_line(writer: &mut dyn Write, line: &str) -> std::io::Result<()> {
    writer.write_all(line.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}

/// Consume `reader` line by line until end-of-stream, routing each message.
fn reader_loop(reader: Box<dyn Read + Send>, shared: Arc<Mutex<Shared>>) {
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => {
                fail_all(
                    &shared,
                    ProviderError::Exited {
                        status: None,
                        message: "the LPP provider closed its output stream (process exited or connection ended)"
                            .to_string(),
                    },
                );
                return;
            }
            Ok(_) => {
                let message = line.trim_end_matches(['\r', '\n']);
                if message.is_empty() {
                    continue;
                }
                if !dispatch_line(&shared, message) {
                    return;
                }
            }
        }
    }
}

/// Route one provider message. Returns `false` when the session is dead and
/// the reader should stop.
fn dispatch_line(shared: &Arc<Mutex<Shared>>, line: &str) -> bool {
    let parsed: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(error) => {
            poison(
                shared,
                ProviderError::Malformed {
                    detail: format!("line is not valid JSON: {error}"),
                },
            );
            return false;
        }
    };
    let Some(object) = parsed.as_object() else {
        poison(
            shared,
            ProviderError::Malformed {
                detail: "provider sent a batch or non-object message".to_string(),
            },
        );
        return false;
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        poison(
            shared,
            ProviderError::Malformed {
                detail: "provider message lacks jsonrpc \"2.0\"".to_string(),
            },
        );
        return false;
    }
    // LPP v1 defines no notifications: a message without an id, or with an
    // id that matches no pending request, is a protocol violation.
    let Some(id) = object.get("id").and_then(Value::as_i64) else {
        poison(
            shared,
            ProviderError::Malformed {
                detail:
                    "provider message has no integer request id (LPP v1 defines no notifications)"
                        .to_string(),
            },
        );
        return false;
    };

    let sender = {
        let mut guard = shared.lock().expect("LPP reader state lock poisoned");
        match guard.pending.remove(&id) {
            Some(sender) => sender,
            None => {
                drop(guard);
                poison(
                    shared,
                    ProviderError::Malformed {
                        detail: format!("provider response id {id} matches no pending request"),
                    },
                );
                return false;
            }
        }
    };

    // The response is attributable to a pending request: payload failures
    // fail that request only and never poison the session.
    let _ = sender.send(response_outcome(object));
    true
}

/// Extract the outcome of an attributable response message.
fn response_outcome(object: &Map<String, Value>) -> Result<Value, ProviderError> {
    match (object.contains_key("result"), object.contains_key("error")) {
        (true, false) => Ok(object.get("result").cloned().unwrap_or(Value::Null)),
        (false, true) => Err(error_from_response(
            object.get("error").cloned().unwrap_or(Value::Null),
        )),
        (true, true) => Err(ProviderError::Malformed {
            detail: "provider response carries both result and error".to_string(),
        }),
        (false, false) => Err(ProviderError::Malformed {
            detail: "provider response carries neither result nor error".to_string(),
        }),
    }
}

/// Convert a wire error object into a structured client failure.
fn error_from_response(error: Value) -> ProviderError {
    let code = error.get("code").and_then(Value::as_i64);
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_string);
    match (code, message) {
        (Some(-32000), Some(message)) => match error.get("data").and_then(|data| data.get("lpp")) {
            Some(lpp) => {
                let kind = lpp
                    .get("kind")
                    .and_then(Value::as_str)
                    .map(LppErrorKind::from_wire)
                    .unwrap_or_else(|| LppErrorKind::Unknown("<missing>".to_string()));
                let details = lpp.get("details").cloned().unwrap_or_else(|| json!({}));
                ProviderError::Lpp(LppError {
                    kind,
                    details,
                    message,
                })
            }
            None => ProviderError::Malformed {
                detail: format!("LPP error code -32000 without data.lpp: {message}"),
            },
        },
        (Some(code), Some(message)) => ProviderError::JsonRpc { code, message },
        _ => ProviderError::Malformed {
            detail: format!("provider error response is malformed: {error}"),
        },
    }
}

/// Record the first session-poisoning violation and fail every pending
/// request with it.
fn poison(shared: &Arc<Mutex<Shared>>, error: ProviderError) {
    let mut shared = shared.lock().expect("LPP reader state lock poisoned");
    if shared.violation.is_none() {
        shared.violation = Some(error.clone());
        for (_, sender) in shared.pending.drain() {
            let _ = sender.send(Err(error.clone()));
        }
    }
}

/// Record that the provider connection ended and fail every pending request.
fn fail_all(shared: &Arc<Mutex<Shared>>, error: ProviderError) {
    let mut shared = shared.lock().expect("LPP reader state lock poisoned");
    if shared.exited.is_none() {
        shared.exited = Some(error.clone());
        for (_, sender) in shared.pending.drain() {
            let _ = sender.send(Err(error.clone()));
        }
    }
}
