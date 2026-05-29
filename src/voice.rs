/// WebSocket client that connects MicroClaw to the ownify-voice-rtc sidecar.
///
/// Protocol (JSON frames):
///   MicroClaw → voice-rtc:
///     { "type": "answer",    "call_id": "...", "sdp_offer": "..." }
///     { "type": "candidate", "call_id": "...", "candidate": "...", "sdpMid": "...", "sdpMLineIndex": 0 }
///     { "type": "hangup",    "call_id": "..." }
///     { "type": "respond",   "call_id": "...", "text": "..." }
///
///   voice-rtc → MicroClaw:
///     { "type": "sdp_answer",  "call_id": "...", "sdp": "..." }
///     { "type": "candidate",   "call_id": "...", "candidate": "...", "sdpMid": "...", "sdpMLineIndex": 0 }
///     { "type": "transcript",  "call_id": "...", "text": "...", "is_final": true }
///     { "type": "call_ended",  "call_id": "...", "reason": "..." }
///     { "type": "error",       "call_id": "...", "message": "..." }

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
                if let Err(e) = ws_tx.send(Message::Text(json.into())).await {
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
