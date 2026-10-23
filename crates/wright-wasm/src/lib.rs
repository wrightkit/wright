//! The WebAssembly Wright core (#959, ADR-0037): `ToolService` over
//! in-memory Workshop text behind a JavaScript-callable C ABI — a
//! `wright-agent/v1` request body in, the `ToolService` response body
//! out, nothing wrapped and no new protocol contract.
//!
//! The browser host holds the in-memory source text, the core holds the
//! session; the host write path (`wright_apply`) re-validates the
//! `EditTransaction` an `editStatements` result produced, rewrites the
//! held text, and reloads — mirroring `wright ui`'s `/apply` on disk
//! (ADR-0032 decision 5).

use std::collections::BTreeMap;
use std::path::PathBuf;

use wright_driver::edit::EditTransaction;
use wright_driver::service::{ServiceState, ToolRequest, ToolService};
use wright_driver::{CompilerSession, InputSpec, SessionConfig};

/// A core owns its session and the `ServiceState` a `ToolService` carries
/// between requests — `ToolService` borrows the session, so the core
/// resumes a fresh service per call and stores its state back (#959).
/// JavaScript drives the exported entry points; Rust callers (the native
/// parity driver, tests) call the same methods directly.
pub struct WrightCore {
    session: CompilerSession,
    state: Option<ServiceState>,
}

impl WrightCore {
    /// A core over caller-held Workshop text. `name` gives the input its
    /// file identity — kind detection, root resolution, and span paths
    /// behave exactly like an on-disk file of that name (`InputSpec::Text`,
    /// #555); on WebAssembly relative names resolve against the virtual
    /// root `/`.
    pub fn new(text: String, name: &str) -> Result<WrightCore, String> {
        let config = SessionConfig {
            input: InputSpec::Text {
                text,
                path: Some(PathBuf::from(name)),
            },
            ..SessionConfig::default()
        };
        let mut session = CompilerSession::new(config).map_err(|diagnostic| diagnostic.message)?;
        let state = ToolService::new(&mut session)
            .map_err(|diagnostic| diagnostic.message)?
            .into_state();
        Ok(WrightCore {
            session,
            state: Some(state),
        })
    }

    /// One `wright-agent/v1` request body in, the `ToolService` response
    /// body out — identical bytes to what `serve`/`ui` write on the wire.
    pub fn request(&mut self, body: &str) -> String {
        let request: ToolRequest = match serde_json::from_str(body) {
            Ok(request) => request,
            Err(error) => {
                return serde_json::json!({ "error": { "code": "malformed-request", "message": error.to_string() } })
                    .to_string();
            }
        };
        self.with_service(|service| {
            serde_json::to_string(&service.handle(&request)).expect("a response serializes")
        })
        .unwrap_or_else(|diagnostic| error_body(&diagnostic))
    }

    /// The ADR-0037 host write path: apply `transaction` — an
    /// `editStatements` result — to `sources`, the host's current
    /// in-memory text keyed by source name. The answer is the
    /// `EditValidation` payload `wright ui`'s `/apply` returns plus the
    /// authoritative `sources` map after the call: on refusal every
    /// identity recheck failed and nothing changed; on success the map
    /// carries the rewritten text the core now serves.
    pub fn apply(&mut self, transaction_body: &str, sources_body: &str) -> String {
        let transaction: EditTransaction = match serde_json::from_str(transaction_body) {
            Ok(transaction) => transaction,
            Err(_) => {
                return serde_json::json!({ "error": { "code": "malformed-request", "message": "the apply body is not an edit transaction" } })
                    .to_string();
            }
        };
        let mut sources: BTreeMap<String, String> = match serde_json::from_str(sources_body) {
            Ok(sources) => sources,
            Err(_) => {
                return serde_json::json!({ "error": { "code": "malformed-request", "message": "the apply body is not a sources map" } })
                    .to_string();
            }
        };
        let result = self.with_service(|service| {
            let validation = service.apply_text_edit_transaction(&transaction, &mut sources);
            let mut body =
                serde_json::to_value(&validation).expect("an edit validation serializes");
            body["sources"] = serde_json::to_value(&sources).expect("a sources map serializes");
            body.to_string()
        });
        result.unwrap_or_else(|diagnostic| error_body(&diagnostic))
    }

    fn with_service<R>(
        &mut self,
        call: impl FnOnce(&mut ToolService) -> R,
    ) -> Result<R, Box<wright_driver::Diagnostic>> {
        let state = match self.state.take() {
            Some(state) => state,
            // A call interrupted mid-service leaves no state behind;
            // rebuilding is the same construction `new` performs.
            None => ToolService::new(&mut self.session)
                .map_err(Box::new)?
                .into_state(),
        };
        let mut service = ToolService::resume(&mut self.session, state).map_err(Box::new)?;
        let result = call(&mut service);
        self.state = Some(service.into_state());
        Ok(result)
    }
}

/// The diagnostic `ToolService::resume` can only produce is a refused
/// mutation; render it in the response-body vocabulary.
fn error_body(diagnostic: &wright_driver::Diagnostic) -> String {
    serde_json::json!({ "error": { "code": diagnostic.code, "message": diagnostic.message } })
        .to_string()
}

/// The JavaScript-callable entry points (ADR-0037 decision 2). Every
/// string crosses the boundary as `(ptr, len)` UTF-8; return values are
/// packed `ptr << 32 | len` into a `u64`. The host allocates input
/// buffers with `wright_alloc` and frees every buffer — its own and the
/// returned one — with `wright_dealloc`.
#[cfg(target_arch = "wasm32")]
mod abi {
    use std::cell::RefCell;

    use super::WrightCore;

    thread_local! {
        static CORE: RefCell<Option<WrightCore>> = const { RefCell::new(None) };
    }

    /// # Safety
    ///
    /// `ptr` must name `len` readable bytes of the instance's memory for
    /// the call's duration.
    #[allow(unsafe_code)]
    unsafe fn text<'a>(ptr: *const u8, len: u32) -> Result<&'a str, String> {
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
        std::str::from_utf8(bytes).map_err(|error| error.to_string())
    }

    fn packed(body: String) -> u64 {
        // `into_boxed_slice` shrinks to `len` — `wright_dealloc` frees the
        // buffer as `len` bytes, so the allocation must be exactly that.
        let bytes = body.into_bytes().into_boxed_slice();
        let len = bytes.len() as u64;
        let ptr = Box::into_raw(bytes) as *mut u8 as u64;
        (ptr << 32) | len
    }

    fn malformed(message: impl Into<String>) -> u64 {
        packed(
            serde_json::json!({ "error": { "code": "malformed-request", "message": message.into() } })
                .to_string(),
        )
    }

    /// Reserve `len` bytes of wasm memory; the host writes its string
    /// there and hands the pointer to an entry point.
    ///
    /// # Safety
    ///
    /// The returned pointer is valid for `len` bytes until
    /// `wright_dealloc` releases it.
    #[allow(unsafe_code)]
    #[unsafe(no_mangle)]
    pub(crate) unsafe extern "C" fn wright_alloc(len: u32) -> *mut u8 {
        // A boxed slice's allocation is exactly `len` — matching the
        // layout `wright_dealloc` frees.
        Box::into_raw(vec![0u8; len as usize].into_boxed_slice()) as *mut u8
    }

    /// Release a buffer `wright_alloc` reserved or an entry point
    /// returned.
    ///
    /// # Safety
    ///
    /// `ptr`/`len` must name a live allocation of exactly `len` bytes.
    #[allow(unsafe_code)]
    #[unsafe(no_mangle)]
    pub(crate) unsafe extern "C" fn wright_dealloc(ptr: *mut u8, len: u32) {
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len as usize)) });
    }

    /// Create the core over `(source_ptr, source_len)` Workshop text
    /// named `(name_ptr, name_len)`; the answer is a JSON body —
    /// `{"result":{...}}` on success, `{"error":{...}}` on refusal.
    ///
    /// # Safety
    ///
    /// Both pointer/length pairs must name readable wasm memory.
    #[allow(unsafe_code)]
    #[unsafe(no_mangle)]
    pub(crate) unsafe extern "C" fn wright_new(
        source_ptr: *const u8,
        source_len: u32,
        name_ptr: *const u8,
        name_len: u32,
    ) -> u64 {
        let (source, name) = unsafe {
            match (text(source_ptr, source_len), text(name_ptr, name_len)) {
                (Ok(source), Ok(name)) => (source, name),
                (Err(error), _) | (_, Err(error)) => return malformed(error),
            }
        };
        match WrightCore::new(source.to_string(), name) {
            Ok(core) => {
                CORE.with(|slot| *slot.borrow_mut() = Some(core));
                packed(
                    serde_json::json!({ "result": { "contract": wright_driver::service::AGENT_CONTRACT } })
                        .to_string(),
                )
            }
            Err(message) => malformed(message),
        }
    }

    /// One `wright-agent/v1` request body at `(body_ptr, body_len)`; the
    /// answer is the `ToolService` response body — `{"result":...}` or
    /// `{"error":{...}}`.
    ///
    /// # Safety
    ///
    /// The pointer/length pair must name readable wasm memory and the
    /// core must exist (`wright_new` succeeded).
    #[allow(unsafe_code)]
    #[unsafe(no_mangle)]
    pub(crate) unsafe extern "C" fn wright_request(body_ptr: *const u8, body_len: u32) -> u64 {
        let body = unsafe {
            match text(body_ptr, body_len) {
                Ok(body) => body,
                Err(error) => return malformed(error),
            }
        };
        CORE.with(|slot| match slot.borrow_mut().as_mut() {
            Some(core) => packed(core.request(body)),
            None => malformed("the core does not exist — call wright_new first"),
        })
    }

    /// The host write path: the `EditTransaction` body at
    /// `(transaction_ptr, transaction_len)` applied to the host's current
    /// sources map at `(sources_ptr, sources_len)`; the answer is the
    /// `EditValidation` body plus the authoritative `sources`.
    ///
    /// # Safety
    ///
    /// Both pointer/length pairs must name readable wasm memory and the
    /// core must exist.
    #[allow(unsafe_code)]
    #[unsafe(no_mangle)]
    pub(crate) unsafe extern "C" fn wright_apply(
        transaction_ptr: *const u8,
        transaction_len: u32,
        sources_ptr: *const u8,
        sources_len: u32,
    ) -> u64 {
        let (transaction, sources) = unsafe {
            match (
                text(transaction_ptr, transaction_len),
                text(sources_ptr, sources_len),
            ) {
                (Ok(transaction), Ok(sources)) => (transaction, sources),
                (Err(error), _) | (_, Err(error)) => return malformed(error),
            }
        };
        CORE.with(|slot| match slot.borrow_mut().as_mut() {
            Some(core) => packed(core.apply(transaction, sources)),
            None => malformed("the core does not exist — call wright_new first"),
        })
    }
}
