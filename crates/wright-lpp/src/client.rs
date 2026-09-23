use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::error::{LppError, LppErrorKind, ProviderError};

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub request_timeout: Duration,
}

impl Default for ClientConfig {
    fn default() -> Self {
        ClientConfig {
            request_timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientPhase {
    Fresh,
    Ready,
    ShutDown,
}

#[derive(Default)]
struct Shared {
    pending: HashMap<i64, Sender<Result<Value, ProviderError>>>,
    violation: Option<ProviderError>,
    exited: Option<ProviderError>,
}

pub struct JsonRpcClient {
    writer: Option<Box<dyn Write + Send>>,
    shared: Arc<Mutex<Shared>>,
    _reader: Option<JoinHandle<()>>,
    next_id: i64,
    phase: ClientPhase,
    timeout: Duration,
}

impl JsonRpcClient {
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
            .expect("spawning reader thread cannot fail");
        JsonRpcClient {
            writer: Some(writer),
            shared,
            _reader: Some(reader_thread),
            next_id: 0,
            phase: ClientPhase::Fresh,
            timeout: config.request_timeout,
        }
    }

    pub fn phase(&self) -> ClientPhase {
        self.phase
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    pub fn initialize(&mut self, params: Value) -> Result<Value, ProviderError> {
        match self.phase {
            ClientPhase::Fresh => {}
            ClientPhase::Ready => return Err(ProviderError::AlreadyInitialized),
            ClientPhase::ShutDown => {
                return Err(ProviderError::ShutDown {
                    method: "lpp/initialize".into(),
                });
            }
        }
        let value = self.send("lpp/initialize", params)?;
        self.phase = ClientPhase::Ready;
        Ok(value)
    }

    pub fn request(&mut self, method: &str, params: Value) -> Result<Value, ProviderError> {
        match self.phase {
            ClientPhase::Fresh => {
                return Err(ProviderError::NotInitialized {
                    method: method.into(),
                });
            }
            ClientPhase::ShutDown => {
                return Err(ProviderError::ShutDown {
                    method: method.into(),
                });
            }
            ClientPhase::Ready => {}
        }
        self.send(method, params)
    }

    pub fn shutdown(&mut self) -> Result<(), ProviderError> {
        match self.phase {
            ClientPhase::Fresh => {
                return Err(ProviderError::NotInitialized {
                    method: "lpp/shutdown".into(),
                });
            }
            ClientPhase::ShutDown => {
                return Err(ProviderError::ShutDown {
                    method: "lpp/shutdown".into(),
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

    pub fn close_stdin(&mut self) {
        self.writer = None;
    }

    pub fn reset_initialize(&mut self) {
        if matches!(self.phase, ClientPhase::Ready) {
            self.phase = ClientPhase::Fresh;
        }
    }

    fn send(&mut self, method: &str, params: Value) -> Result<Value, ProviderError> {
        {
            let shared = self.shared.lock().expect("lock poisoned");
            if let Some(err) = shared.violation.as_ref().or(shared.exited.as_ref()) {
                return Err(err.clone());
            }
        }

        self.next_id += 1;
        let id = self.next_id;
        let (tx, rx) = mpsc::channel();
        {
            let mut shared = self.shared.lock().expect("lock poisoned");
            if let Some(err) = shared.violation.as_ref().or(shared.exited.as_ref()) {
                return Err(err.clone());
            }
            shared.pending.insert(id, tx);
        }

        let line =
            json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string();
        let writer = self.writer.as_mut().ok_or_else(|| ProviderError::Io {
            message: format!("cannot send '{method}': the provider stdin is closed"),
        })?;
        if let Err(error) = write_line(writer.as_mut(), &line) {
            let shared = self.shared.lock().expect("lock poisoned");
            if let Some(exited) = &shared.exited {
                return Err(exited.clone());
            }
            return Err(ProviderError::Io {
                message: format!("cannot write request '{method}': {error}"),
            });
        }

        match rx.recv_timeout(self.timeout) {
            Ok(res) => res,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(ProviderError::Timeout {
                method: method.to_string(),
                duration: self.timeout,
            }),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let shared = self.shared.lock().expect("lock poisoned");
                if let Some(err) = shared.violation.as_ref().or(shared.exited.as_ref()) {
                    return Err(err.clone());
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

fn write_line(writer: &mut dyn Write, line: &str) -> std::io::Result<()> {
    writer.write_all(line.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}

fn reader_loop(reader: Box<dyn Read + Send>, shared: Arc<Mutex<Shared>>) {
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => {
                terminate_shared(&shared, false, ProviderError::Exited {
                    status: None,
                    message: "the LPP provider closed its output stream (process exited or connection ended)".to_string(),
                });
                return;
            }
            Ok(_) => {
                let msg = line.trim_end_matches(['\r', '\n']);
                if msg.is_empty() {
                    continue;
                }
                if !dispatch_line(&shared, msg) {
                    return;
                }
            }
        }
    }
}

fn dispatch_line(shared: &Arc<Mutex<Shared>>, line: &str) -> bool {
    let parsed: Value = match serde_json::from_str(line) {
        Ok(val) => val,
        Err(e) => {
            terminate_shared(
                shared,
                true,
                ProviderError::Malformed {
                    detail: format!("line is not valid JSON: {e}"),
                },
            );
            return false;
        }
    };
    let Some(object) = parsed.as_object() else {
        terminate_shared(
            shared,
            true,
            ProviderError::Malformed {
                detail: "provider sent a batch or non-object message".into(),
            },
        );
        return false;
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        terminate_shared(
            shared,
            true,
            ProviderError::Malformed {
                detail: "provider message lacks jsonrpc \"2.0\"".into(),
            },
        );
        return false;
    }
    let Some(id) = object.get("id").and_then(Value::as_i64) else {
        terminate_shared(
            shared,
            true,
            ProviderError::Malformed {
                detail:
                    "provider message has no integer request id (LPP v1 defines no notifications)"
                        .into(),
            },
        );
        return false;
    };

    let sender = {
        let mut guard = shared.lock().expect("lock poisoned");
        match guard.pending.remove(&id) {
            Some(s) => s,
            None => {
                drop(guard);
                terminate_shared(
                    shared,
                    true,
                    ProviderError::Malformed {
                        detail: format!("provider response id {id} matches no pending request"),
                    },
                );
                return false;
            }
        }
    };

    let _ = sender.send(response_outcome(object));
    true
}

fn response_outcome(object: &Map<String, Value>) -> Result<Value, ProviderError> {
    match (object.contains_key("result"), object.contains_key("error")) {
        (true, false) => Ok(object.get("result").cloned().unwrap_or(Value::Null)),
        (false, true) => Err(error_from_response(
            object.get("error").cloned().unwrap_or(Value::Null),
        )),
        (true, true) => Err(ProviderError::Malformed {
            detail: "provider response carries both result and error".into(),
        }),
        (false, false) => Err(ProviderError::Malformed {
            detail: "provider response carries neither result nor error".into(),
        }),
    }
}

fn error_from_response(error: Value) -> ProviderError {
    let code = error.get("code").and_then(Value::as_i64);
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_string);
    match (code, message) {
        (Some(-32000), Some(message)) => match error.get("data").and_then(|d| d.get("lpp")) {
            Some(lpp) => {
                let kind = lpp
                    .get("kind")
                    .and_then(Value::as_str)
                    .map(LppErrorKind::from_wire)
                    .unwrap_or_else(|| LppErrorKind::Unknown("<missing>".into()));
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

fn terminate_shared(shared: &Arc<Mutex<Shared>>, is_violation: bool, error: ProviderError) {
    let mut guard = shared.lock().expect("lock poisoned");
    let slot = if is_violation {
        &mut guard.violation
    } else {
        &mut guard.exited
    };
    if slot.is_none() {
        *slot = Some(error.clone());
        for (_, sender) in guard.pending.drain() {
            let _ = sender.send(Err(error.clone()));
        }
    }
}
