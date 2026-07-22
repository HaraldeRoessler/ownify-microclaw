// src/web/a2a.rs
//
// Inbound A2A REST endpoint — the gateway calls this to forward A2A
// messages to microclaw's agent loop. The gateway handles all A2A
// protocol (JSON-RPC 2.0, Agent Card, Task Store). Microclaw only
// needs the internal /api/a2a/message route so the gateway can
// deliver inbound messages to the agent.

use super::*;
use crate::a2a::{
    default_session_key_for_source, local_agent_name, sanitize_for_json,
    A2AMessageRequest, A2AMessageResponse, A2A_PROTOCOL_VERSION,
};

/// Tool allowlist for external (non-tenant) A2A callers. Set when the
/// gateway forwards `x-ownify-caller-kind: external`.
const EXTERNAL_A2A_TOOLS: &[&str] = &[
    "web_search",
    "web_fetch",
    "get_current_time",
    "compare_time",
    "calculate",
    "read_memory",
    "write_memory",
];

const EXTERNAL_A2A_ALWAYS_ON: &[&str] = &["read_memory", "write_memory"];

fn allowed_tools_for_caller(headers: &HeaderMap) -> Option<Vec<String>> {
    let kind = headers
        .get("x-ownify-caller-kind")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .unwrap_or("");
    if !kind.eq_ignore_ascii_case("external") {
        return None;
    }

    let grants_raw = headers
        .get("x-ownify-caller-grants")
        .and_then(|v| v.to_str().ok());
    let Some(grants_raw) = grants_raw else {
        return Some(EXTERNAL_A2A_TOOLS.iter().map(|s| s.to_string()).collect());
    };

    let granted: std::collections::HashSet<&str> = grants_raw
        .split(',')
        .map(str::trim)
        .filter_map(|cap| cap.strip_prefix("invoke_tool:"))
        .collect();

    let mut allow: Vec<String> = EXTERNAL_A2A_TOOLS
        .iter()
        .filter(|name| {
            EXTERNAL_A2A_ALWAYS_ON.contains(name) || granted.contains(*name)
        })
        .map(|s| s.to_string())
        .collect();
    allow.sort();
    Some(allow)
}

fn a2a_token_allowed(config: &Config, headers: &HeaderMap) -> bool {
    let Some(raw) = headers.get("authorization").and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let raw = raw.trim();
    let mut parts = raw.splitn(2, char::is_whitespace);
    let Some(scheme) = parts.next() else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("bearer") {
        return false;
    };
    let Some(token) = parts.next().map(str::trim).filter(|v| !v.is_empty()) else {
        return false;
    };
    config
        .a2a
        .shared_tokens
        .iter()
        .any(|candidate| candidate == token)
}

/// POST /api/a2a/message — internal endpoint called by the a2a-gateway.
///
/// The gateway authenticates via Bearer token (shared_token from a2a
/// config). This handler forwards the message to the agent loop and
/// returns the response. The gateway wraps this into an A2A v1.0.0
/// Task for the wire format.
pub(super) async fn api_a2a_message(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<A2AMessageRequest>,
) -> Result<Json<A2AMessageResponse>, (StatusCode, String)> {
    metrics_http_inc(&state).await;
    if !state.app_state.config.a2a.enabled {
        return Err((StatusCode::NOT_FOUND, "A2A is disabled".into()));
    }
    if state.app_state.config.a2a.shared_tokens.is_empty() {
        return Err((
            StatusCode::FORBIDDEN,
            "A2A inbound auth is not configured".into(),
        ));
    }
    if !a2a_token_allowed(&state.app_state.config, &headers) {
        return Err((StatusCode::UNAUTHORIZED, "invalid A2A bearer token".into()));
    }

    let message = sanitize_for_json(body.message.trim());
    if message.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "message is required".into()));
    }
    let caller_did: Option<String> = headers
        .get("x-ownify-caller-did")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty() && v.starts_with("did:"))
        .map(|v| v.to_string());

    let session_key = body
        .session_key
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            caller_did.as_ref().map(|did| format!("a2a:{did}"))
        })
        .unwrap_or_else(|| default_session_key_for_source(body.source_agent.as_deref()));

    let message = if let Some(ref did) = caller_did {
        format!("[A2A from: {}] {}", did, message)
    } else {
        message
    };
    let sender_name = body
        .sender_name
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            body.source_agent
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(|v| format!("a2a:{v}"))
        })
        .unwrap_or_else(|| "a2a-remote".to_string());

    let allowed_tools = allowed_tools_for_caller(&headers);
    let images: Option<Vec<(String, String)>> = body.images.as_ref().map(|imgs| {
        imgs.iter()
            .map(|i| (i.base64.clone(), i.mime.clone()))
            .filter(|(b, m)| !b.is_empty() && !m.is_empty())
            .collect()
    });
    let result = super::send_and_store_response(
        state.clone(),
        super::SendRequest {
            session_key: Some(session_key.clone()),
            sender_name: Some(sender_name),
            message,
            allowed_tools,
            images,
        },
    )
    .await?;
    let payload = result.0;
    let response = payload
        .get("response")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let resolved_session_key = payload
        .get("session_key")
        .and_then(|v| v.as_str())
        .unwrap_or(&session_key)
        .to_string();

    audit_log(
        &state,
        "a2a",
        body.source_agent.as_deref().unwrap_or("a2a-peer"),
        "a2a.message",
        Some(&resolved_session_key),
        "ok",
        body.source_url.as_deref(),
    )
    .await;

    Ok(Json(A2AMessageResponse {
        ok: true,
        protocol_version: A2A_PROTOCOL_VERSION.to_string(),
        agent_name: local_agent_name(&state.app_state.config),
        session_key: resolved_session_key,
        response,
    }))
}