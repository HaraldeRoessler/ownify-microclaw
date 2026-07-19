// Art. 12 audit sink client — sends local audit events to the central
// ownify-control-plane /api/internal/audit-sink endpoint so they appear
// in the portal's audit trail alongside router/gateway/CP events.
//
// Fire-and-forget: failures are logged but never block the agent loop.
// The local SQLite audit_logs table remains the source of truth — this
// is a secondary copy for centralized viewing.
//
// Config via env vars:
//   AUDIT_SINK_URL  — CP audit-sink endpoint (e.g. http://ownify-control-plane.ownify-control-plane.svc.cluster.local/api/internal/audit-sink)
//   OWNIFY_TENANT_SLUG — tenant slug (already used elsewhere in microclaw)

use std::sync::Arc;
use tokio::sync::Mutex;

const QUEUE_MAX: usize = 500;

pub struct AuditSink {
    queue: Arc<Mutex<Vec<AuditEvent>>>,
    url: String,
    slug: String,
    flushing: Arc<Mutex<bool>>,
}

#[derive(serde::Serialize, Clone)]
struct AuditEvent {
    kind: String,
    action: String,
    target: Option<String>,
    status: String,
    detail: String,
    at: String,
}

impl Default for AuditSink {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditSink {
    pub fn new() -> Self {
        let url = std::env::var("AUDIT_SINK_URL").unwrap_or_default();
        let slug = std::env::var("OWNIFY_TENANT_SLUG").unwrap_or_else(|_| "unknown".to_string());
        Self {
            queue: Arc::new(Mutex::new(Vec::new())),
            url,
            slug,
            flushing: Arc::new(Mutex::new(false)),
        }
    }

    pub fn is_enabled(&self) -> bool {
        !self.url.is_empty()
    }

    pub async fn enqueue(&self, kind: &str, action: &str, target: Option<&str>, status: &str, detail: &str) {
        if !self.is_enabled() {
            return;
        }
        let event = AuditEvent {
            kind: kind.to_string(),
            action: action.to_string(),
            target: target.map(|s| s.to_string()),
            status: status.to_string(),
            detail: detail.to_string(),
            at: chrono::Utc::now().to_rfc3339(),
        };
        let mut q = self.queue.lock().await;
        if q.len() >= QUEUE_MAX {
            q.remove(0);
        }
        q.push(event);
        drop(q);
        self.flush().await;
    }

    async fn flush(&self) {
        let mut flushing = self.flushing.lock().await;
        if *flushing {
            return;
        }
        *flushing = true;
        drop(flushing);

        loop {
            let event = {
                let mut q = self.queue.lock().await;
                if q.is_empty() {
                    break;
                }
                q.remove(0)
            };

            let body = serde_json::json!({
                "slug": self.slug,
                "kind": event.kind,
                "source": "microclaw",
                "detail": serde_json::from_str::<serde_json::Value>(&event.detail)
                    .unwrap_or(serde_json::json!({"raw": event.detail})),
            });

            let client = match reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
            {
                Ok(c) => c,
                Err(_) => continue,
            };

            match client.post(&self.url).json(&body).send().await {
                Ok(r) if !r.status().is_success() => {
                    tracing::warn!("audit sink non-2xx: {}", r.status());
                }
                Err(e) => {
                    tracing::warn!("audit sink failed: {}", e);
                }
                _ => {}
            }
        }

        let mut flushing = self.flushing.lock().await;
        *flushing = false;
    }
}