//! ownify-fork: outbound DLP scanner client. Phase 3k v1.
//!
//! Implements `microclaw_channels::egress_filter::EgressFilter` against
//! the cluster-side ownify-egress-scanner service. The scanner does the
//! pattern matching and decides allow / redact / refuse / alert.
//!
//! Configuration (env, read at boot):
//!   OWNIFY_EGRESS_SCAN_URL   — base URL of the cluster-side egress scanning
//!                              service. Unset → no filter installed.
//!   OWNIFY_EGRESS_SCAN_TOKEN — optional Bearer presented to /scan.
//!   OWNIFY_TENANT_SLUG       — tenant slug, recorded in audit rows.
//!   OWNIFY_EGRESS_FAIL_OPEN  — "1" / "true" → allow on scanner outage.
//!                              Default = fail-closed (refuse) so a scanner
//!                              outage can't silently disable DLP.
//!
//! Backwards compatibility: if the OWNIFY_* variant is unset but the
//! legacy KLAW_* variant is set, the latter is read with a one-time
//! deprecation warning. This lets operators upgrade microclaw before /
//! after the control-plane KLAW→OWNIFY rebrand, in either order.

use async_trait::async_trait;
use microclaw_channels::egress_filter::{EgressDecision, EgressFilter};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, warn};

#[derive(Clone)]
pub struct ScannerEgressFilter {
    base_url: String,
    bearer: Option<String>,
    slug: Option<String>,
    http: reqwest::Client,
    fail_open: bool,
}

#[derive(Serialize)]
struct ScanRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    slug: Option<&'a str>,
    channel: &'a str,
    body: &'a str,
}

#[derive(Deserialize)]
struct ScanHit {
    class: String,
    #[allow(dead_code)]
    count: u32,
    #[allow(dead_code)]
    sample_hash: String,
}

#[derive(Deserialize)]
struct ScanResponse {
    action: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    hits: Vec<ScanHit>,
}

/// Read an env var preferring the OWNIFY_* name; fall back to the
/// legacy KLAW_* name with a one-time deprecation warning. Empty
/// values are treated as unset.
fn read_env(ownify_name: &str, klaw_name: &str) -> Option<String> {
    if let Ok(v) = std::env::var(ownify_name) {
        if !v.trim().is_empty() {
            return Some(v);
        }
    }
    match std::env::var(klaw_name) {
        Ok(v) if !v.trim().is_empty() => {
            warn!(
                "{klaw_name} is deprecated — set {ownify_name} instead. \
                 Reading legacy value for backwards compatibility."
            );
            Some(v)
        }
        _ => None,
    }
}

impl ScannerEgressFilter {
    /// Build the filter from process env. Returns None if OWNIFY_EGRESS_SCAN_URL
    /// (or the legacy KLAW_EGRESS_SCAN_URL) is unset — caller should skip
    /// installing the filter in that case.
    pub fn from_env() -> Option<Arc<dyn EgressFilter>> {
        let base_url = read_env("OWNIFY_EGRESS_SCAN_URL", "KLAW_EGRESS_SCAN_URL")?;
        if base_url.trim().is_empty() {
            return None;
        }
        let bearer = read_env("OWNIFY_EGRESS_SCAN_TOKEN", "KLAW_EGRESS_SCAN_TOKEN");
        let slug = read_env("OWNIFY_TENANT_SLUG", "KLAW_TENANT_SLUG");
        let fail_open = read_env("OWNIFY_EGRESS_FAIL_OPEN", "KLAW_EGRESS_FAIL_OPEN")
            .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);
        let http = reqwest::Client::builder()
            // Scanner is in-cluster and fast; a short timeout avoids
            // stalling outbound delivery if the scanner is degraded.
            .timeout(Duration::from_secs(3))
            .build()
            .ok()?;
        Some(Arc::new(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            bearer,
            slug,
            http,
            fail_open,
        }))
    }
}

#[async_trait]
impl EgressFilter for ScannerEgressFilter {
    async fn screen(&self, channel_name: &str, text: &str) -> EgressDecision {
        let url = format!("{}/scan", self.base_url);
        let payload = ScanRequest {
            slug: self.slug.as_deref(),
            channel: channel_name,
            body: text,
        };
        let mut req = self.http.post(&url).json(&payload);
        if let Some(b) = self.bearer.as_ref() {
            req = req.bearer_auth(b);
        }
        let resp = match req.send().await {
            Ok(r) => r,
            Err(err) => {
                if self.fail_open {
                    warn!(error = %err, "egress scanner unreachable, fail-open allow");
                    return EgressDecision::Allow;
                }
                warn!(error = %err, "egress scanner unreachable, fail-closed refuse");
                return EgressDecision::Refuse {
                    reason: format!("egress scanner unreachable: {err}"),
                };
            }
        };
        if !resp.status().is_success() {
            let status = resp.status();
            if self.fail_open {
                warn!(%status, "egress scanner non-2xx, fail-open allow");
                return EgressDecision::Allow;
            }
            warn!(%status, "egress scanner non-2xx, fail-closed refuse");
            return EgressDecision::Refuse {
                reason: format!("egress scanner non-2xx: {status}"),
            };
        }
        let body: ScanResponse = match resp.json().await {
            Ok(b) => b,
            Err(err) => {
                if self.fail_open {
                    warn!(error = %err, "egress scanner bad response, fail-open allow");
                    return EgressDecision::Allow;
                }
                return EgressDecision::Refuse {
                    reason: format!("egress scanner bad response: {err}"),
                };
            }
        };
        let classes: Vec<String> = body.hits.iter().map(|h| h.class.clone()).collect();
        match body.action.as_str() {
            "allow" => {
                debug!(channel = channel_name, "egress allow");
                EgressDecision::Allow
            }
            "redact" => {
                debug!(channel = channel_name, classes = ?classes, "egress redact");
                match body.body {
                    Some(replaced) => EgressDecision::Redact { body: replaced },
                    None => EgressDecision::Refuse {
                        reason: "scanner returned redact without body".to_string(),
                    },
                }
            }
            "alert" | "refuse" => {
                warn!(
                    channel = channel_name,
                    classes = ?classes,
                    action = body.action.as_str(),
                    "egress blocked"
                );
                EgressDecision::Refuse {
                    reason: format!("egress {} ({})", body.action, classes.join(",")),
                }
            }
            other => {
                warn!(action = other, "unknown egress action, refusing");
                EgressDecision::Refuse {
                    reason: format!("unknown egress action: {other}"),
                }
            }
        }
    }
}
