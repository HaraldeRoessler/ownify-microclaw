use std::sync::Arc;

use serde_json::Value;

use crate::traces::{kv, kv_int, new_span_id, new_trace_id, now_unix_nano, OtlpTraceExporter, SpanData};

/// Collects per-turn phase payloads and flushes them as a single
/// OTLP span via the existing trace exporter pipeline.
///
/// Instantiated once per agent turn.  If `MEMORY_LOOP_TRACE` is not
/// set the struct is a no-op — every call is a cheap branch.
pub struct LoopTrace {
    session_id: String,
    turn_id: u64,
    exporter: Option<Arc<OtlpTraceExporter>>,
    phases: Vec<(String, Value)>,
    start_time_unix_nano: u64,
    enabled: bool,
}

impl LoopTrace {
    /// Create a new trace collector for one agent turn.
    ///
    /// `exporter` is the **existing** `OtlpTraceExporter` that the
    /// agent engine already holds.  The trace will reuse that
    /// exporter's OTLP connection and batch processor.
    pub fn new(
        session_id: &str,
        turn_id: u64,
        exporter: Arc<OtlpTraceExporter>,
    ) -> Self {
        let enabled = std::env::var("MEMORY_LOOP_TRACE").is_ok();
        Self {
            session_id: session_id.to_string(),
            turn_id,
            exporter: Some(exporter),
            phases: Vec::new(),
            start_time_unix_nano: now_unix_nano(),
            enabled,
        }
    }

    /// Internal constructor used by unit tests — exporter is optional.
    #[cfg(test)]
    fn new_with_optional_exporter(
        session_id: &str,
        turn_id: u64,
        exporter: Option<Arc<OtlpTraceExporter>>,
        enabled: bool,
    ) -> Self {
        Self {
            session_id: session_id.to_string(),
            turn_id,
            exporter,
            phases: Vec::new(),
            start_time_unix_nano: 0,
            enabled,
        }
    }

    /// Record a named phase with an arbitrary JSON payload.
    ///
    /// When the env var is unset this call returns immediately.
    pub fn record(&mut self, phase_name: &str, payload: Value) {
        if self.enabled {
            self.phases.push((phase_name.to_string(), payload));
        }
    }

    /// Emit the collected phases as a single `memory_loop_trace` span.
    ///
    /// All phases are serialised into a JSON array stored in the
    /// `phases` attribute.  The span is routed through the same OTLP
    /// exporter that the agent engine already uses for `agent_run`
    /// spans.
    ///
    /// When disabled, or no exporter is attached, this is a no-op.
    pub fn flush(self) {
        if !self.enabled || self.phases.is_empty() {
            return;
        }

        let Some(exporter) = self.exporter else {
            return;
        };

        let end_time = now_unix_nano();
        let phases_json = serde_json::to_string(
            &self
                .phases
                .iter()
                .map(|(name, payload)| {
                    serde_json::json!({"phase": name, "payload": payload})
                })
                .collect::<Vec<_>>(),
        )
        .unwrap_or_default();

        exporter.send_span(SpanData {
            trace_id: new_trace_id(),
            span_id: new_span_id(),
            parent_span_id: vec![],
            name: "memory_loop_trace".to_string(),
            start_time_unix_nano: self.start_time_unix_nano,
            end_time_unix_nano: end_time,
            attributes: vec![
                kv("session_id", &self.session_id),
                kv_int("turn_id", self.turn_id as i64),
                kv_int("phase_count", self.phases.len() as i64),
                kv("phases", &phases_json),
            ],
            status: None,
            kind: 1,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_two_phases_increments_vec() {
        let mut trace =
            LoopTrace::new_with_optional_exporter("sess-1", 42, None, true);

        trace.record("phase_a", serde_json::json!({"key": "value"}));
        trace.record("phase_b", serde_json::json!({"count": 3}));

        assert_eq!(trace.phases.len(), 2);
        assert_eq!(trace.phases[0].0, "phase_a");
        assert_eq!(trace.phases[1].0, "phase_b");
    }

    #[test]
    fn disabled_is_noop() {
        let mut trace =
            LoopTrace::new_with_optional_exporter("sess-1", 42, None, false);

        trace.record("phase_a", serde_json::json!({"key": "val"}));
        assert!(trace.phases.is_empty());
    }

    #[test]
    fn disabled_flush_returns_early() {
        let trace =
            LoopTrace::new_with_optional_exporter("sess-1", 42, None, false);
        trace.flush();
    }

    #[test]
    fn empty_flush_without_exporter_is_noop() {
        let trace =
            LoopTrace::new_with_optional_exporter("sess-1", 42, None, true);
        trace.flush();
    }
}
