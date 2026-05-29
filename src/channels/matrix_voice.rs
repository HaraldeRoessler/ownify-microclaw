/// Matrix Voice Call Handler — REST endpoints for voice-rtc sidecar.

use std::sync::Arc;
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::agent_engine::{process_with_agent, AgentRequestContext};
use crate::runtime::AppState;

// ── WebState extractor helper ────────────────────────────────────

/// The web module uses `WebState`, not `Arc<AppState>`. This wrapper
/// extracts `AppState` from the web state struct.
pub(crate) fn app_state(
    ws: &crate::web::WebState,
) -> Arc<AppState> {
    ws.app_state.clone()
}

// ── REST API types ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct TranscriptRequest {
    pub call_id: String,
    pub room_id: String,
    pub sender: String,
    pub text: String,
    #[serde(default)]
    pub matrix_token: String,
}

#[derive(Debug, Serialize)]
pub struct TranscriptResponse {
    pub status: String,
    pub agent_response: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct VoiceEventRequest {
    pub call_id: String,
    pub room_id: String,
    pub sender: String,
    pub event_type: String,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct VoiceEventResponse {
    pub status: String,
}

// ── Handlers ────────────────────────────────────────────────────

/// Receive transcribed voice from voice-rtc → feed to agent loop.
pub async fn handle_transcript(
    State(ws): State<crate::web::WebState>,
    Json(req): Json<TranscriptRequest>,
) -> Json<TranscriptResponse> {
    let state = app_state(&ws);

    info!(
        "Voice transcript from {sender} in room {room}: \"{text}\"",
        sender = req.sender,
        room = req.room_id,
        text = &req.text
    );

    let chat_id = hash_to_i64(&req.call_id);

    let context = AgentRequestContext {
        caller_channel: "matrix-voice",
        chat_id,
        chat_type: "direct",
        allowed_tools: None,
    };

    let prompt = format!("[Voice Call from {}]: {}", req.sender, req.text);

    let result = process_with_agent(
        &state,
        context,
        Some(&prompt),
        None,
    ).await;

    let agent_response = result.ok().filter(|t| !t.trim().is_empty());

    Json(TranscriptResponse {
        status: "ok".into(),
        agent_response,
    })
}

/// Receive voice call lifecycle event.
pub async fn handle_voice_event(
    State(_ws): State<crate::web::WebState>,
    Json(req): Json<VoiceEventRequest>,
) -> Json<VoiceEventResponse> {
    info!(
        "Voice event: {event} for call {call_id} (reason: {reason:?})",
        event = req.event_type,
        call_id = req.call_id,
        reason = req.reason
    );

    Json(VoiceEventResponse { status: "ok".into() })
}

// ── Helpers ─────────────────────────────────────────────────────

fn hash_to_i64(s: &str) -> i64 {
    use std::hash::{Hash, Hasher};
    use std::collections::hash_map::DefaultHasher;
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    (h.finish() & 0x1FFFFFFFFFFFFF) as i64
}
