//! WebSocket client that connects MicroClaw to the ownify-voice-rtc sidecar.
//!
//! Protocol (JSON frames):
//!   MicroClaw → voice-rtc:
//!     { "type": "answer",    "call_id": "...", "sdp_offer": "..." }
//!     { "type": "candidate", "call_id": "...", "candidate": "...", "sdpMid": "...", "sdpMLineIndex": 0 }
//!     { "type": "hangup",    "call_id": "..." }
//!     { "type": "respond",   "call_id": "...", "text": "..." }
//!
//!   voice-rtc → MicroClaw:
//!     { "type": "sdp_answer",  "call_id": "...", "sdp": "..." }
//!     { "type": "candidate",   "call_id": "...", "candidate": "...", "sdpMid": "...", "sdpMLineIndex": 0 }
//!     { "type": "transcript",  "call_id": "...", "text": "...", "is_final": true }
//!     { "type": "call_ended",  "call_id": "...", "reason": "..." }
//!     { "type": "error",       "call_id": "...", "message": "..." }

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

/// Messages we send to voice-rtc
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum VoiceCommand {
    #[serde(rename = "answer")]
    Answer {
        call_id: String,
        sdp_offer: String,
    },
    #[serde(rename = "candidate")]
    Candidate {
        call_id: String,
        candidate: String,
        #[serde(rename = "sdpMid")]
        sdp_mid: String,
        #[serde(rename = "sdpMLineIndex")]
        sdp_m_line_index: u32,
    },
    #[serde(rename = "hangup")]
    Hangup { call_id: String },
    #[serde(rename = "respond")]
    Respond {
        call_id: String,
        text: String,
    },
}

/// Events we receive from voice-rtc
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum VoiceEvent {
    #[serde(rename = "sdp_answer")]
    SdpAnswer {
        call_id: String,
        sdp: String,
    },
    #[serde(rename = "candidate")]
    Candidate {
        call_id: String,
        candidate: String,
        #[serde(rename = "sdpMid")]
        sdp_mid: Option<String>,
        #[serde(rename = "sdpMLineIndex")]
        sdp_m_line_index: Option<u32>,
    },
    #[serde(rename = "transcript")]
    Transcript {
        call_id: String,
        text: String,
        is_final: bool,
    },
    #[serde(rename = "call_ended")]
    CallEnded {
        call_id: String,
        reason: Option<String>,
    },
    #[serde(rename = "error")]
    Error {
        call_id: Option<String>,
        message: String,
    },
}

/// Callback invoked when voice-rtc emits an event.
pub type VoiceEventHandler = Box<dyn Fn(VoiceEvent) + Send + Sync>;

/// Manages the WebSocket connection to the ownify-voice-rtc sidecar.
pub struct VoiceRtcClient {
    tx: mpsc::UnboundedSender<VoiceCommand>,
    event_handlers: Arc<RwLock<Vec<VoiceEventHandler>>>,
}

impl VoiceRtcClient {
    /// Connect to voice-rtc at the given WebSocket URL and start the receive loop.
    pub async fn connect(url: &str) -> Result<Self, anyhow::Error> {
        let (ws_stream, _) = connect_async(url).await?;
        let (mut ws_tx, mut ws_rx) = ws_stream.split();

        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<VoiceCommand>();
        let event_handlers: Arc<RwLock<Vec<VoiceEventHandler>>> = Arc::new(RwLock::new(Vec::new()));
        let handlers = event_handlers.clone();

        // Spawn task: forward commands to WebSocket
        tokio::spawn(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                let json = serde_json::to_string(&cmd).unwrap();
                if let Err(e) = ws_tx.send(Message::Text(json)).await {
                    error!("voice-rtc send error: {e}");
                    break;
                }
            }
        });

        // Spawn task: receive events from WebSocket
        tokio::spawn(async move {
            while let Some(msg) = ws_rx.next().await {
                let text = match msg {
                    Ok(Message::Text(t)) => t.to_string(),
                    Ok(Message::Close(_)) => {
                        info!("voice-rtc connection closed");
                        break;
                    }
                    Err(e) => {
                        error!("voice-rtc receive error: {e}");
                        break;
                    }
                    _ => continue,
                };

                match serde_json::from_str::<VoiceEvent>(&text) {
                    Ok(event) => {
                        let handlers = handlers.read().await;
                        for handler in handlers.iter() {
                            handler(event.clone());
                        }
                    }
                    Err(e) => {
                        warn!("voice-rtc unparseable event: {e} — raw: {text}");
                    }
                }
            }
        });

        Ok(Self {
            tx: cmd_tx,
            event_handlers,
        })
    }

    /// Register an event handler. Called before any calls start.
    pub async fn on_event(&self, handler: VoiceEventHandler) {
        self.event_handlers.write().await.push(handler);
    }

    /// Send an answer command (with remote SDP offer from Matrix).
    pub fn answer(&self, call_id: &str, sdp_offer: &str) {
        let _ = self.tx.send(VoiceCommand::Answer {
            call_id: call_id.to_string(),
            sdp_offer: sdp_offer.to_string(),
        });
    }

    /// Forward a remote ICE candidate from Matrix.
    pub fn add_ice_candidate(&self, call_id: &str, candidate: &str, sdp_mid: &str, sdp_m_line_index: u32) {
        let _ = self.tx.send(VoiceCommand::Candidate {
            call_id: call_id.to_string(),
            candidate: candidate.to_string(),
            sdp_mid: sdp_mid.to_string(),
            sdp_m_line_index,
        });
    }

    /// Hang up a call.
    pub fn hangup(&self, call_id: &str) {
        let _ = self.tx.send(VoiceCommand::Hangup {
            call_id: call_id.to_string(),
        });
    }

    /// Ask voice-rtc to speak text (TTS → remote audio).
    pub fn respond(&self, call_id: &str, text: &str) {
        let _ = self.tx.send(VoiceCommand::Respond {
            call_id: call_id.to_string(),
            text: text.to_string(),
        });
    }
}

/// Active call state kept by the Matrix voice handler.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveCall {
    pub call_id: String,
    pub room_id: String,
    pub sender: String,       // Matrix user ID who called
    pub state: CallState,
    pub last_transcript: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CallState {
    Ringing,
    Connecting,
    Connected,
    Ended,
}

/// Registry of active voice calls, shared across threads.
pub struct CallRegistry {
    calls: RwLock<HashMap<String, ActiveCall>>,
}

impl Default for CallRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl CallRegistry {
    pub fn new() -> Self {
        Self {
            calls: RwLock::new(HashMap::new()),
        }
    }

    pub async fn insert(&self, call: ActiveCall) {
        self.calls.write().await.insert(call.call_id.clone(), call);
    }

    pub async fn get(&self, call_id: &str) -> Option<ActiveCall> {
        self.calls.read().await.get(call_id).cloned()
    }

    pub async fn update_state(&self, call_id: &str, state: CallState) {
        if let Some(call) = self.calls.write().await.get_mut(call_id) {
            call.state = state;
        }
    }

    pub async fn remove(&self, call_id: &str) -> Option<ActiveCall> {
        self.calls.write().await.remove(call_id)
    }

    pub async fn list_active(&self) -> Vec<ActiveCall> {
        self.calls
            .read()
            .await
            .values()
            .filter(|c| matches!(c.state, CallState::Connected | CallState::Connecting | CallState::Ringing))
            .cloned()
            .collect()
    }

    pub async fn find_by_room(&self, room_id: &str) -> Option<String> {
        self.calls
            .read()
            .await
            .iter()
            .find(|(_, c)| c.room_id == room_id && c.state != CallState::Ended)
            .map(|(id, _)| id.clone())
    }
}

// =============================================================================
// STT / TTS dispatch (merged from upstream microclaw v0.2.2)
//
// Upstream's `voice.rs` was rewritten to be a shared voice/audio dispatch layer
// for the channel adapters (Telegram voice, Discord attachment, Slack file,
// Feishu audio message → STT provider, and TTS for round-trip audio replies).
// Our fork's WebSocket client (above) coexists with this dispatch layer.
// =============================================================================

use std::path::PathBuf;

use crate::config::Config;

/// Returns true if a transcription provider is configured. Channels can use
/// this to decide between transcribing or surfacing a "voice not supported"
/// notice to the sender.
pub fn can_transcribe(config: &Config) -> bool {
    if config.voice_provider == "local" {
        config.voice_transcription_command.is_some()
    } else {
        config.openai_api_key.is_some()
    }
}

/// Run audio bytes through the configured STT provider.
pub async fn transcribe_audio(config: &Config, audio_bytes: &[u8]) -> Result<String, String> {
    let provider = &config.voice_provider;

    if provider == "local" {
        let Some(ref command) = config.voice_transcription_command else {
            return Err(
                "Local voice transcription configured but voice_transcription_command not set"
                    .into(),
            );
        };

        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join(format!("voice_{}.ogg", uuid::Uuid::new_v4()));
        tokio::fs::write(&temp_file, audio_bytes)
            .await
            .map_err(|e| e.to_string())?;

        let cmd = command.replace("{file}", temp_file.to_str().unwrap_or(""));

        let output_result = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .output()
            .await;

        let _ = tokio::fs::remove_file(&temp_file).await;

        let output =
            output_result.map_err(|e| format!("Failed to run transcription command: {e}"))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(format!(
                "Transcription command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
        }
    } else {
        let Some(ref openai_key) = config.openai_api_key else {
            return Err("Voice transcription requires openai_api_key".into());
        };
        microclaw_app::transcribe::transcribe_audio(openai_key, audio_bytes).await
    }
}

/// Standard inbound formatting so the agent always sees voice messages with
/// the same shape regardless of platform.
pub fn format_voice_inbound(sender_name: &str, transcription: &str) -> String {
    format!("[voice message from {sender_name}]: {transcription}")
}

/// Standard error shape when transcription was attempted but failed.
pub fn format_voice_inbound_error(sender_name: &str, error: &str) -> String {
    format!("[voice message from {sender_name}]: [transcription failed: {error}]")
}

/// True when this deployment should reply with audio to voice-inbound turns.
/// Requires both the operator opt-in (`voice_round_trip: true`) and the TTS
/// layer to be enabled in `media.tts`.
pub fn round_trip_enabled(config: &Config) -> bool {
    config.voice_round_trip && config.media.tts.enabled
}

/// Synthesize `text` to a temporary audio file using the configured TTS
/// provider, returning the on-disk path. Caller is responsible for sending
/// it to the user and removing the file when done.
///
/// Bypasses the `text_to_speech` tool surface so channel adapters don't
/// have to fabricate a tool-input shape just to play back a reply.
pub async fn synth_speech_to_temp(config: &Config, text: &str) -> Result<PathBuf, String> {
    if !round_trip_enabled(config) {
        return Err("voice_round_trip is disabled".into());
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("empty text".into());
    }
    // The OpenAI /audio/speech endpoint caps input at 4096 chars. Truncate
    // rather than fail — partial audio is more useful than none.
    let payload_text: String = trimmed.chars().take(4096).collect();

    let media = &config.media;
    let tts = &media.tts;
    let api_key = media
        .api_key
        .as_deref()
        .or(config.openai_api_key.as_deref())
        .ok_or_else(|| "media.api_key (or openai_api_key) not set".to_string())?;
    let base_url = media
        .base_url
        .clone()
        .or_else(|| config.openai_base_url.clone())
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("http client build failed: {e}"))?;
    let url = format!("{}/audio/speech", base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": tts.model,
        "voice": tts.default_voice,
        "input": payload_text,
        "response_format": tts.default_format,
    });
    let resp = client
        .post(&url)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("audio/speech request failed: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("audio/speech HTTP {status}: {body}"));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("audio body read failed: {e}"))?;

    let temp_dir = std::env::temp_dir();
    let path = temp_dir.join(format!(
        "microclaw_reply_{}.{}",
        uuid::Uuid::new_v4(),
        tts.default_format
    ));
    tokio::fs::write(&path, &bytes)
        .await
        .map_err(|e| format!("failed to write temp audio: {e}"))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_requires_both_flags() {
        let mut cfg = Config::test_defaults();
        // Default state: TTS disabled and round_trip false → gate is closed.
        assert!(!round_trip_enabled(&cfg));

        cfg.voice_round_trip = true;
        // Round-trip on but TTS still disabled → gate stays closed; we never
        // want to surprise an operator who hasn't opted into TTS billing.
        assert!(!round_trip_enabled(&cfg));

        cfg.media.tts.enabled = true;
        // Both flags on → gate opens.
        assert!(round_trip_enabled(&cfg));
    }

    #[test]
    fn synth_returns_err_when_round_trip_disabled() {
        let cfg = Config::test_defaults();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = rt
            .block_on(synth_speech_to_temp(&cfg, "hi"))
            .expect_err("should refuse when disabled");
        assert!(err.contains("disabled"));
    }

    #[test]
    fn format_voice_inbound_uses_brackets_so_agent_can_distinguish() {
        let s = format_voice_inbound("alice", "ship the patch");
        assert_eq!(s, "[voice message from alice]: ship the patch");
    }
}

