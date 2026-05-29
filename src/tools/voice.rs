//! Voice call tools for the ownify agent.
//!
//! Tools:
//!   - voice_speak: Send a text-to-speech response in an active call
//!   - voice_hangup: Hang up an active call
//!   - voice_status: Get status of all active calls

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use futures_util::SinkExt;

use microclaw_core::llm_types::ToolDefinition;
use microclaw_tools::runtime::{Tool, ToolResult};

use crate::config::Config;

// ── Helper: send command to voice-rtc WebSocket ─────────────────

async fn send_voice_command(command: &Value, voice_rtc_url: &str) -> Result<(), String> {
    let (mut ws, _) = connect_async(voice_rtc_url)
        .await
        .map_err(|e| format!("Failed to connect to voice-rtc: {e}"))?;

    let json = serde_json::to_string(command)
        .map_err(|e| format!("Serialization error: {e}"))?;

    ws.send(Message::Text(json))
        .await
        .map_err(|e| format!("Send error: {e}"))?;

    ws.close(None).await.ok();
    Ok(())
}

// ── voice_speak ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct VoiceSpeakInput {
    pub call_id: String,
    pub text: String,
}

pub struct VoiceSpeakTool {
    voice_rtc_url: String,
}

impl VoiceSpeakTool {
    pub fn new(config: &Config) -> Self {
        Self {
            voice_rtc_url: config.voice_rtc_url.clone(),
        }
    }
}

#[async_trait]
impl Tool for VoiceSpeakTool {
    fn name(&self) -> &str { "voice_speak" }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "voice_speak".into(),
            description: "Speak text to a live voice call using text-to-speech. Use this to respond verbally to someone who called you via Matrix voice. The call_id must be from an active voice call (check voice_status first).".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "call_id": {
                        "type": "string",
                        "description": "The call ID of the active voice call"
                    },
                    "text": {
                        "type": "string",
                        "description": "The text to speak (will be converted to speech via TTS). Keep it concise for voice."
                    }
                },
                "required": ["call_id", "text"]
            }),
        }
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let params: VoiceSpeakInput = match serde_json::from_value(input) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid input: {e}")),
        };

        let command = json!({
            "type": "speak",
            "call_id": params.call_id,
            "text": params.text,
        });

        match send_voice_command(&command, &self.voice_rtc_url).await {
            Ok(()) => {
                let preview = &params.text[..params.text.len().min(80)];
                ToolResult::success(format!("Speaking: \"{}\"", preview))
            }
            Err(e) => ToolResult::error(e),
        }
    }
}

// ── voice_hangup ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct VoiceHangupInput {
    pub call_id: String,
    #[serde(default)]
    pub reason: Option<String>,
}

pub struct VoiceHangupTool {
    voice_rtc_url: String,
}

impl VoiceHangupTool {
    pub fn new(config: &Config) -> Self {
        Self {
            voice_rtc_url: config.voice_rtc_url.clone(),
        }
    }
}

#[async_trait]
impl Tool for VoiceHangupTool {
    fn name(&self) -> &str { "voice_hangup" }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "voice_hangup".into(),
            description: "Hang up an active voice call. Use this to end a Matrix voice call when the conversation is finished.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "call_id": {
                        "type": "string",
                        "description": "The call ID of the voice call to hang up"
                    },
                    "reason": {
                        "type": "string",
                        "description": "Optional reason for hanging up"
                    }
                },
                "required": ["call_id"]
            }),
        }
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let params: VoiceHangupInput = match serde_json::from_value(input) {
            Ok(p) => p,
            Err(e) => return ToolResult::error(format!("Invalid input: {e}")),
        };

        let command = json!({
            "type": "hangup",
            "call_id": params.call_id,
            "reason": params.reason.unwrap_or_else(|| "agent_requested".to_string()),
        });

        match send_voice_command(&command, &self.voice_rtc_url).await {
            Ok(()) => ToolResult::success(format!("Call {} hung up", params.call_id)),
            Err(e) => ToolResult::error(e),
        }
    }
}

// ── voice_status ────────────────────────────────────────────────

pub struct VoiceStatusTool {
    voice_rtc_health_url: String,
}

impl VoiceStatusTool {
    pub fn new(config: &Config) -> Self {
        Self {
            voice_rtc_health_url: config.voice_rtc_health_url.clone(),
        }
    }
}

#[async_trait]
impl Tool for VoiceStatusTool {
    fn name(&self) -> &str { "voice_status" }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "voice_status".into(),
            description: "Check the status of the voice system and any active voice calls. Returns how many calls are active and whether Matrix VoIP is connected.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        }
    }

    async fn execute(&self, _input: Value) -> ToolResult {
        match reqwest::get(&self.voice_rtc_health_url).await {
            Ok(resp) => {
                if let Ok(body) = resp.json::<Value>().await {
                    let calls = body.get("calls").and_then(|v| v.as_u64()).unwrap_or(0);
                    let matrix_ok = body.get("matrix_connected").and_then(|v| v.as_bool()).unwrap_or(false);
                    ToolResult::success(format!(
                        "Voice system is healthy. Matrix VoIP: {}. Active calls: {}.",
                        if matrix_ok { "connected" } else { "disconnected" },
                        calls
                    ))
                } else {
                    ToolResult::success("Voice system status unknown (could not parse health response)".into())
                }
            }
            Err(e) => ToolResult::error(format!("Could not reach voice-rtc: {e}")),
        }
    }
}
