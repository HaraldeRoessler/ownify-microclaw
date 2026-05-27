use super::*;
use crate::a2a::{
    build_agent_card, default_session_key_for_source, local_agent_name, sanitize_for_json,
    A2AMessageRequest, A2AMessageResponse, A2ATaskRequest, A2ATaskResponse,
    A2ATaskStatusResponse, A2A_PROTOCOL_VERSION,
};
use axum::extract::Query;

/// Tool allowlist for external (non-tenant) A2A callers. Set when the
/// gateway forwards `x-ownify-caller-kind: external`. Deliberately
/// narrow: read-only public-information tools, time/math, plus
/// CHAT-scoped memory so the agent can remember things visitor-by-
/// visitor. The memory tools self-fence the global + bot scopes for
/// external callers (see ReadMemoryTool/WriteMemoryTool::execute);
/// chat scope is per-session_key (= per-visitor cookie when called
/// via the external caller's signed widget) so visitor A's writes never leak to
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

/// Memory tools are always available to external callers at the schema
/// level — `tools/memory.rs` enforces chat-scope only inside the tool
/// itself, so per-visitor isolation already comes for free. The Phase C
/// strict-per-tool fence covers `invoke_tool:*`; memory wing-scoping is
/// Component 4.
const EXTERNAL_A2A_ALWAYS_ON: &[&str] = &["read_memory", "write_memory"];

/// Build the per-call tool allowlist for an A2A inbound request.
///
/// Returns `None` for the historical full-trust path (no
/// `x-ownify-caller-kind` header, or kind != external).
///
/// Returns `Some(Vec<String>)` for fenced external callers. The set is
/// derived as follows:
///   - `x-ownify-caller-grants` header **absent** → fallback to the
///     full `EXTERNAL_A2A_TOOLS` surface (Phase B back-compat for
///     gateway versions before Component 2 / CP fetch failures).
///   - Header **present** → strict per-tool mode.
///     allowed = (invoke_tool:* entries from header ∩ EXTERNAL_A2A_TOOLS)
///     ∪ EXTERNAL_A2A_ALWAYS_ON.
///     Capabilities outside EXTERNAL_A2A_TOOLS (e.g. admin granted
///     `invoke_tool:sendgrid` to an external caller) are silently
///     dropped — the runtime fence is the source of truth, not the
///     grant table. read_memory:* entries are deferred to Component 4.
fn allowed_tools_for_caller(headers: &HeaderMap) -> Option<Vec<String>> {
    let kind = headers
        .get("x-ownify-caller-kind")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .unwrap_or("");
    if !kind.eq_ignore_ascii_case("external") {
        return None;
    }

    // Header presence (not value) is the toggle between back-compat and
    // strict mode. Absent = old gateway / CP fetch failed / fail-soft.
    let grants_raw = headers
        .get("x-ownify-caller-grants")
        .and_then(|v| v.to_str().ok());
    let Some(grants_raw) = grants_raw else {
        // Back-compat: full external surface.
        return Some(EXTERNAL_A2A_TOOLS.iter().map(|s| s.to_string()).collect());
    };

    // Strict mode. Parse `invoke_tool:<name>` entries; everything else
    // (message, read_memory:*) is irrelevant to invoke_tool gating.
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
    // Deterministic order helps log diffs when debugging.
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

    let message = sanitize_for_json(body.message.trim());
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

// ── A2A Task Create (async delegation) ─────────────────────────────────────

pub(super) async fn api_a2a_task_create(
    headers: HeaderMap,
    State(state): State<WebState>,
    Json(body): Json<A2ATaskRequest>,
) -> Result<Json<A2ATaskResponse>, (StatusCode, String)> {
    metrics_http_inc(&state).await;
    if !state.app_state.config.a2a.enabled {
        return Err((StatusCode::NOT_FOUND, "A2A is disabled".into()));
    }
    if state.app_state.config.a2a.shared_tokens.is_empty() {
        return Err((StatusCode::FORBIDDEN, "A2A inbound auth not configured".into()));
    }
    if !a2a_token_allowed(&state.app_state.config, &headers) {
        return Err((StatusCode::UNAUTHORIZED, "invalid A2A bearer token".into()));
    }
    let task_text = sanitize_for_json(body.task.trim());
    if task_text.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "task is required".into()));
    }
    let source_agent = body.source_agent.as_deref().unwrap_or("a2a-remote").to_string();
    let task_id = format!("a2a-{}-{}",
        &source_agent[..source_agent.len().min(16)],
        chrono::Utc::now().timestamp());
    let db = state.app_state.db.clone();
    let prompt = format!("[A2A-TASK from {}]\n\n{}", source_agent, task_text);
    let now = chrono::Utc::now().to_rfc3339();
    let _task_id: i64 = match db.create_scheduled_task(0, &prompt, "once", &now, &now) {
        Ok(id) => id,
        Err(e) => return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("failed to queue task: {e}"))),
    };
    let task_id_str = format!("a2a-{}", _task_id);

    audit_log(&state, "a2a", &source_agent, "a2a.task.create", Some(&task_id_str), "ok", body.source_url.as_deref()).await;
    Ok(Json(A2ATaskResponse {
        status: "accepted".to_string(),
        task_id,
    }))
}

// ── A2A Task Status ─────────────────────────────────────────────────────────

pub(super) async fn api_a2a_task_status(
    headers: HeaderMap,
    Query(params): Query<std::collections::HashMap<String, String>>,
    State(state): State<WebState>,
) -> Result<Json<A2ATaskStatusResponse>, (StatusCode, String)> {
    metrics_http_inc(&state).await;
    if !state.app_state.config.a2a.enabled {
        return Err((StatusCode::NOT_FOUND, "A2A is disabled".into()));
    }
    if !a2a_token_allowed(&state.app_state.config, &headers) {
        return Err((StatusCode::UNAUTHORIZED, "invalid A2A bearer token".into()));
    }
    let Some(task_id) = params.get("task_id").map(|s| s.as_str()) else {
        return Err((StatusCode::BAD_REQUEST, "task_id query parameter is required".into()));
    };
    let db = state.app_state.db.clone();
    let task_id_int: i64 = task_id.parse().map_err(|_| (StatusCode::BAD_REQUEST, "invalid task_id".into()))?;
    let (status_str, result_opt) = match db.get_task_status_for_a2a(task_id_int) {
        Ok(Some((status, result))) => (status, result),
        Ok(None) => return Err((StatusCode::NOT_FOUND, format!("task {task_id} not found"))),
        Err(e) => return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("db error: {e}"))),
    };
    audit_log(&state, "a2a", "a2a-caller", "a2a.task.status", Some(task_id), "ok", None).await;
    Ok(Json(A2ATaskStatusResponse {
        task_id: task_id.to_string(),
        status: status_str,
        result: result_opt,
    }))
}
