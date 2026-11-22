use crate::trace::Trace;

/// The built-in catalog is embedded data, so a load error is a build-time
/// defect and panics instead of reaching a diagnostic channel.
pub(super) fn builtin_catalog() -> &'static workshop_rs::catalog::Catalog {
    workshop_rs::catalog::Catalog::builtin_ref()
        .unwrap_or_else(|error| panic!("cannot load the built-in Workshop catalog: {error}"))
}

/// Overlap the catalog load with the work that follows construction, such as
/// the provider start (#1068). The span ends when the load ends.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn start_catalog_load(trace: &Trace) {
    let span = trace.parallel_span("catalog.load");
    std::thread::spawn(move || {
        builtin_catalog();
        drop(span);
    });
}

/// `wasm32` has no threads: the load stays synchronous.
#[cfg(target_arch = "wasm32")]
pub(super) fn start_catalog_load(trace: &Trace) {
    let _span = trace.span("catalog.load");
    builtin_catalog();
}
