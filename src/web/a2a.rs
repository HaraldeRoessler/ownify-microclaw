use super::*;
use crate::a2a::{
    build_agent_card, default_session_key_for_source, local_agent_name, A2AMessageRequest,
    A2AMessageResponse, A2A_PROTOCOL_VERSION,
};

/// Tool allowlist for external (non-tenant) A2A callers. Set when the
/// gateway forwards `x-ownify-caller-kind: external`. Deliberately
/// narrow: read-only public-information tools, time/math, plus
/// CHAT-scoped memory so the agent can remember things visitor-by-
/// visitor. The memory tools self-fence the global + bot scopes for
/// external callers (see ReadMemoryTool/WriteMemoryTool::execute);
/// chat scope is per-session_key (= per-visitor cookie when called
/// via dsncon's signed widget) so visitor A's writes never leak to
/// visitor B's reads.
///
/// Excluded by construction: bash, file I/O, peer a2a_send, the
/// scheduler, sub-agents, skill management, send_message, and any
/// MCP-server-provided tool (mcp_*) that points at tenant-shared
/// storage.
const EXTERNAL_A2A_TOOLS: &[&str] = &[
    "web_search",
    "web_fetch",
    "get_current_time",
    "compare_time",
    "calculate",
    "read_memory",   // chat-scope only — non-chat scopes refused inside the tool
    "write_memory",  // chat-scope only — non-chat scopes refused inside the tool
];

/// Read the `x-ownify-caller-kind` header set by ownify-a2a-gateway.
/// Currently recognises `external` only — anything else (or missing)
/// resolves to None and the call gets the full tool surface, the
/// historical behaviour matrix/web/scheduler relied on.
fn allowed_tools_for_caller(headers: &HeaderMap) -> Option<&'static [&'static str]> {
    let kind = headers
        .get("x-ownify-caller-kind")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .unwrap_or("");
    if kind.eq_ignore_ascii_case("external") {
        Some(EXTERNAL_A2A_TOOLS)
    } else {
        None
    }
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

pub(super) async fn api_a2a_agent_card(
    State(state): State<WebState>,
) -> Result<Json<crate::a2a::A2AAgentCard>, (StatusCode, String)> {
    metrics_http_inc(&state).await;
    if !state.app_state.config.a2a.enabled {
        return Err((StatusCode::NOT_FOUND, "A2A is disabled".into()));
    }
    Ok(Json(build_agent_card(&state.app_state.config)))
}

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

    let message = body.message.trim().to_string();
    if message.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "message is required".into()));
    }
    let session_key = body
        .session_key
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| default_session_key_for_source(body.source_agent.as_deref()));
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
    let result = super::send_and_store_response(
        state.clone(),
        super::SendRequest {
            session_key: Some(session_key.clone()),
            sender_name: Some(sender_name),
            message,
            allowed_tools,
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
