//! The opt-in execution trace (#1079): a span tree with monotonic timings
//! and key facts, recorded across one workflow. A disabled [`Trace`] holds
//! no recorder, so every span is a no-op and never reads the clock. The
//! CLI owns rendering; this module only records.

use std::sync::{Arc, Mutex};
use std::time::Instant;

/// A key fact attached to a span or to the trace environment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fact {
    Count(u64),
    Text(String),
}

impl From<u64> for Fact {
    fn from(value: u64) -> Fact {
        Fact::Count(value)
    }
}

impl From<usize> for Fact {
    fn from(value: usize) -> Fact {
        Fact::Count(value as u64)
    }
}

impl From<&str> for Fact {
    fn from(value: &str) -> Fact {
        Fact::Text(value.to_string())
    }
}

impl From<String> for Fact {
    fn from(value: String) -> Fact {
        Fact::Text(value)
    }
}

/// A completed span as the renderer reads it. Times are offsets from the
/// trace origin, in nanoseconds; `parent` is the enclosing span's id.
#[derive(Clone, Debug)]
pub struct SpanReport {
    pub id: usize,
    pub parent: Option<usize>,
    pub name: &'static str,
    pub start_ns: u64,
    pub duration_ns: u64,
    pub failed: bool,
    pub facts: Vec<(&'static str, Fact)>,
}

/// A snapshot of one trace: the environment facts and every span in start
/// order. The first span is the root.
#[derive(Clone, Debug, Default)]
pub struct TraceReport {
    pub environment: Vec<(&'static str, Fact)>,
    pub spans: Vec<SpanReport>,
}

#[derive(Default)]
struct SpanData {
    name: &'static str,
    parent: Option<usize>,
    started: Option<Instant>,
    finished: Option<Instant>,
    failed: bool,
    facts: Vec<(&'static str, Fact)>,
}

#[derive(Default)]
struct State {
    spans: Vec<SpanData>,
    open: Vec<usize>,
    environment: Vec<(&'static str, Fact)>,
}

struct Recorder {
    origin: Instant,
    state: Mutex<State>,
}

impl Recorder {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn offset_ns(&self, instant: Instant) -> u64 {
        u64::try_from(instant.saturating_duration_since(self.origin).as_nanos()).unwrap_or(u64::MAX)
    }
}

/// A handle to one trace. Cloning shares the same recorder.
#[derive(Clone, Default)]
pub struct Trace {
    recorder: Option<Arc<Recorder>>,
}

impl Trace {
    /// A trace that records nothing and costs nothing.
    pub fn disabled() -> Trace {
        Trace { recorder: None }
    }

    /// A trace that records every span from this call onward.
    pub fn enabled() -> Trace {
        Trace {
            recorder: Some(Arc::new(Recorder {
                origin: Instant::now(),
                state: Mutex::new(State::default()),
            })),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.recorder.is_some()
    }

    /// Open a span under the innermost open span. It ends when the guard
    /// drops, so an early return still records it.
    pub fn span(&self, name: &'static str) -> Span {
        self.start_span(name, true)
    }

    /// Open a parallel span under the current parent without nesting later spans.
    pub(crate) fn parallel_span(&self, name: &'static str) -> Span {
        self.start_span(name, false)
    }

    fn start_span(&self, name: &'static str, enter: bool) -> Span {
        let Some(recorder) = &self.recorder else {
            return Span { slot: None };
        };
        let started = Instant::now();
        let mut state = recorder.lock();
        let id = state.spans.len();
        let parent = state.open.last().copied();
        state.spans.push(SpanData {
            name,
            parent,
            started: Some(started),
            ..SpanData::default()
        });
        if enter {
            state.open.push(id);
        }
        drop(state);
        Span {
            slot: Some((Arc::clone(recorder), id)),
        }
    }

    /// Run `work` inside a span and mark the span failed when it returns
    /// `Err`.
    pub fn in_span<T, E>(
        &self,
        name: &'static str,
        work: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        let span = self.span(name);
        let result = work();
        if result.is_err() {
            span.fail();
        }
        result
    }

    /// Record a span whose start and end were measured elsewhere. Its
    /// parent is the innermost open span, so call it while the enclosing
    /// span is still open.
    pub fn record(
        &self,
        name: &'static str,
        started: Instant,
        finished: Instant,
        facts: Vec<(&'static str, Fact)>,
    ) {
        let Some(recorder) = &self.recorder else {
            return;
        };
        let mut state = recorder.lock();
        let parent = state.open.last().copied();
        state.spans.push(SpanData {
            name,
            parent,
            started: Some(started),
            finished: Some(finished),
            failed: false,
            facts,
        });
    }

    /// Set an environment fact. A later value for the same key replaces
    /// the earlier one.
    pub fn set_fact(&self, key: &'static str, value: impl Into<Fact>) {
        let Some(recorder) = &self.recorder else {
            return;
        };
        let value = value.into();
        let mut state = recorder.lock();
        match state.environment.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => state.environment.push((key, value)),
        }
    }

    /// The recorded environment and spans so far, or `None` when disabled.
    pub fn report(&self) -> Option<TraceReport> {
        let recorder = self.recorder.as_ref()?;
        let now = Instant::now();
        let state = recorder.lock();
        let spans = state
            .spans
            .iter()
            .enumerate()
            .map(|(id, span)| {
                let started = span.started.unwrap_or(recorder.origin);
                let finished = span.finished.unwrap_or(now);
                SpanReport {
                    id,
                    parent: span.parent,
                    name: span.name,
                    start_ns: recorder.offset_ns(started),
                    duration_ns: u64::try_from(
                        finished.saturating_duration_since(started).as_nanos(),
                    )
                    .unwrap_or(u64::MAX),
                    failed: span.failed,
                    facts: span.facts.clone(),
                }
            })
            .collect();
        Some(TraceReport {
            environment: state.environment.clone(),
            spans,
        })
    }
}

/// An open span. Dropping it ends the span.
#[must_use = "a span ends when it is dropped; bind it to a variable"]
pub struct Span {
    slot: Option<(Arc<Recorder>, usize)>,
}

impl Span {
    /// Attach a fact to this span.
    pub fn fact(&self, key: &'static str, value: impl Into<Fact>) {
        let Some((recorder, id)) = &self.slot else {
            return;
        };
        let value = value.into();
        recorder.lock().spans[*id].facts.push((key, value));
    }

    /// Mark this span as failed.
    pub fn fail(&self) {
        let Some((recorder, id)) = &self.slot else {
            return;
        };
        recorder.lock().spans[*id].failed = true;
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        let Some((recorder, id)) = self.slot.take() else {
            return;
        };
        let finished = Instant::now();
        let mut state = recorder.lock();
        state.spans[id].finished = Some(finished);
        if let Some(index) = state.open.iter().rposition(|open| *open == id) {
            state.open.remove(index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_trace_records_nothing() {
        let trace = Trace::disabled();
        {
            let span = trace.span("compile");
            span.fact("bytes", 3_usize);
        }
        trace.set_fact("os", "windows");
        assert!(!trace.is_enabled());
        assert!(trace.report().is_none());
    }

    #[test]
    fn spans_nest_by_drop_order_and_keep_failures() {
        let trace = Trace::enabled();
        {
            let root = trace.span("root");
            root.fact("exit", 1_u64);
            let child = trace.in_span("child", || Err::<(), _>("boom"));
            assert!(child.is_err());
            let sibling = trace.span("sibling");
            drop(sibling);
        }
        let report = trace.report().expect("enabled trace reports");
        let names: Vec<_> = report.spans.iter().map(|s| (s.name, s.parent)).collect();
        assert_eq!(
            names,
            vec![("root", None), ("child", Some(0)), ("sibling", Some(0))]
        );
        assert!(report.spans[1].failed);
        assert!(!report.spans[0].failed);
        assert_eq!(report.spans[0].facts, vec![("exit", Fact::Count(1))]);
        assert!(report.spans[0].duration_ns >= report.spans[1].duration_ns);
    }

    #[test]
    fn environment_facts_replace_by_key() {
        let trace = Trace::enabled();
        trace.set_fact("provider", "opy");
        trace.set_fact("provider", "opy-2");
        let report = trace.report().expect("enabled trace reports");
        assert_eq!(
            report.environment,
            vec![("provider", Fact::Text("opy-2".to_string()))]
        );
    }
}
