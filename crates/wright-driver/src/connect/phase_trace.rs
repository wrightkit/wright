use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Deserialize;
use wright_lpp::client::{ExchangeObserver, ExchangeRecord};

use crate::trace::{Fact, Trace};

pub(super) struct ExchangeTrace {
    trace: Trace,
    provider_trace: Option<Mutex<ProviderTrace>>,
}

impl ExchangeTrace {
    pub(super) fn new(trace: &Trace) -> Option<Arc<Self>> {
        trace.is_enabled().then(|| {
            Arc::new(Self {
                trace: trace.clone(),
                provider_trace: tempfile::NamedTempFile::new()
                    .ok()
                    .map(|file| Mutex::new(ProviderTrace { file, offset: 0 })),
            })
        })
    }

    pub(super) fn environment(&self) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
        self.provider_trace
            .as_ref()
            .map(|capture| {
                let capture = capture
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                vec![(
                    "WRIGHT_PROVIDER_TRACE_FILE".into(),
                    capture.file.path().as_os_str().to_owned(),
                )]
            })
            .unwrap_or_default()
    }
}

impl ExchangeObserver for ExchangeTrace {
    fn exchanged(&self, record: &ExchangeRecord<'_>) {
        let mut facts = vec![
            ("method", Fact::from(record.method)),
            ("requestBytes", Fact::from(record.request_bytes)),
            ("responseBytes", Fact::from(record.response_bytes)),
        ];
        if let Some(error) = record.error {
            facts.push(("error", Fact::from(error)));
        }
        let Some(parent) = self
            .trace
            .record("lpp.request", record.started, record.finished, facts)
        else {
            return;
        };
        if let Some(capture) = &self.provider_trace {
            let mut capture = capture
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let _ = capture.read_new(&self.trace, parent, record);
        }
    }
}

struct ProviderTrace {
    file: tempfile::NamedTempFile,
    offset: u64,
}

impl ProviderTrace {
    fn read_new(
        &mut self,
        trace: &Trace,
        parent: usize,
        request: &ExchangeRecord<'_>,
    ) -> std::io::Result<()> {
        let file = File::open(self.file.path())?;
        if file.metadata()?.len() < self.offset {
            self.offset = 0;
        }
        let mut reader = BufReader::new(file);
        reader.seek(SeekFrom::Start(self.offset))?;
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = reader.read_until(b'\n', &mut line)?;
            if read == 0 {
                break;
            }
            self.offset += read as u64;
            if let Ok(record) = serde_json::from_slice::<ProviderRecord>(&line) {
                record.attach(trace, parent, request);
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct ProviderRecord {
    schema: String,
    method: Option<String>,
    spans: Vec<ProviderSpan>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderSpan {
    id: u64,
    parent: Option<u64>,
    name: String,
    start_ns: u64,
    duration_ns: u64,
    facts: BTreeMap<String, Fact>,
}

impl ProviderRecord {
    fn attach(self, trace: &Trace, request_id: usize, request: &ExchangeRecord<'_>) {
        if self.schema != "wright-provider-trace/v1"
            || self.method.as_deref() != Some(request.method)
        {
            return;
        }
        let mut ids = HashSet::new();
        let mut timings = Vec::with_capacity(self.spans.len());
        for span in &self.spans {
            if !span.name.starts_with("provider.")
                || span.parent.is_some_and(|parent| !ids.contains(&parent))
                || !ids.insert(span.id)
            {
                return;
            }
            let Some(started) = request
                .started
                .checked_add(Duration::from_nanos(span.start_ns))
            else {
                return;
            };
            let Some(finished) = started.checked_add(Duration::from_nanos(span.duration_ns)) else {
                return;
            };
            timings.push((started, finished));
        }
        let mut mapped = HashMap::new();
        for (span, (started, finished)) in self.spans.into_iter().zip(timings) {
            let parent = span.parent.map(|id| mapped[&id]).unwrap_or(request_id);
            if let Some(id) = trace.record_child(
                parent,
                span.name,
                started,
                finished,
                span.facts.into_iter().collect(),
            ) {
                mapped.insert(span.id, id);
            }
        }
    }
}
