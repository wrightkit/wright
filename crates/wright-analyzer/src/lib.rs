//! Wright's semantic analysis and agent tooling layer.
//!
//! This crate builds on [`wright_ir`] to expose read-only semantic services
//! over compiled programs (ADR-0006, milestone M4):
//!
//! * [`symbols`] — symbol tables, reference indices, and usage queries;
//! * `cfg` — control-flow graphs and timing-aware primitives;
//! * `analysis` — Workshop-specific static analyses producing findings;
//! * `service` — the transport-neutral read-only tool/agent interface.
//!
//! The crate is protocol-agnostic: it operates on [`wir::Program`], and the
//! `wright-tool` binary wires the pipeline in `wright-core` (protocol →
//! internal HIR → Workshop IR) into these services.

pub mod symbols;
