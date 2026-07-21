use serde::{Deserialize, Serialize};

use crate::config::Config;

/// A2A Protocol v1.0.0 — JSON-RPC 2.0 binding.
/// The gateway handles all A2A protocol; microclaw speaks internal REST
/// with the gateway via /internal/outbound/<peer_did>.
pub const A2A_PROTOCOL_VERSION: &str = "1.0";

// ── A2A v1.0.0 Data Model ──────────────────────────────────────────────

/// A2A v1.0.0 Part — text content (the only kind we support for now).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct A2APart {
    pub kind: String, // "text"
    pub text: String,
}

/// A2A v1.0.0 Artifact — output produced by a completed task.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct A2AArtifact {
    pub parts: Vec<A2APart>,
}

/// A2A v1.0.0 Task Status.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct A2ATaskStatus {
    pub state: String,
    pub timestamp: String,
}

/// A2A v1.0.0 Task — returned by message/send.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct A2ATask {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_id: Option<String>,
    pub status: A2ATaskStatus,
    #[serde(default)]
    pub artifacts: Vec<A2AArtifact>,
}

// ── Internal types (gateway ↔ microclaw REST, unchanged) ──────────────

/// Internal request body for POST /api/a2a/message — the gateway calls
/// this to forward A2A messages to microclaw's agent loop. This is NOT
/// the A2A wire format; it's the internal gateway→backend format.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct A2AMessageRequest {
    #[serde(default)]
    pub session_key: Option<String>,
    #[serde(default)]
    pub sender_name: Option<String>,
    #[serde(default)]
    pub source_agent: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    pub message: String,
    #[serde(default)]
    pub images: Option<Vec<InboundImage>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_did: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_moltrust_did: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_credential: Option<serde_json::Value>,
}

/// Internal response from the gateway's /internal/outbound route.
/// The gateway translates A2A JSON-RPC Task → this simple format for
/// microclaw's a2a_send tool.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct A2AOutboundResponse {
    pub ok: bool,
    #[serde(default)]
    pub response: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Internal response from POST /api/a2a/message — returned by
/// microclaw's web handler to the gateway. This is the backend side;
/// the gateway wraps this into an A2A Task for the wire format.
#[derive(Debug, Serialize, Deserialize)]
pub struct A2AMessageResponse {
    pub ok: bool,
    pub protocol_version: String,
    pub agent_name: String,
    pub session_key: String,
    pub response: String,
}

/// One inbound image attachment in an A2A message.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InboundImage {
    pub base64: String,
    pub mime: String,
}

// ── Helper functions (unchanged) ──────────────────────────────────────

pub fn normalize_peer_name(name: &str) -> Option<String> {
    let trimmed = name.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Find a peer config key by user-provided name — case-insensitive prefix match.
pub fn find_peer<'a>(peers: &'a std::collections::HashMap<String, crate::config::A2APeerConfig>, name: &str) -> Option<&'a crate::config::A2APeerConfig> {
    let key = normalize_peer_name(name)?;
    if let Some(peer) = peers.get(&key) {
        return Some(peer);
    }
    for (k, v) in peers {
        let nk = k.trim().to_ascii_lowercase();
        if nk.starts_with(&key) || nk.split(' ').next() == Some(&key) {
            return Some(v);
        }
    }
    None
}

pub fn normalize_base_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn local_agent_name(config: &Config) -> String {
    config
        .a2a
        .agent_name
        .clone()
        .unwrap_or_else(|| config.bot_username.clone())
}

pub fn effective_base_url(config: &Config) -> Option<String> {
    config.a2a.public_base_url.clone()
}

pub fn sanitize_for_json(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t")
}