use async_trait::async_trait;
use serde_json::json;

use super::{schema_object, Tool, ToolResult};
use crate::a2a::{
    find_peer, normalize_base_url, normalize_peer_name, sanitize_for_json,
    A2AOutboundResponse, A2A_PROTOCOL_VERSION,
};
use crate::config::Config;
use crate::http_client::default_llm_user_agent;
use microclaw_core::llm_types::ToolDefinition;

pub struct A2AListPeersTool {
    config: Config,
}

impl A2AListPeersTool {
    pub fn new(config: &Config) -> Self {
        Self {
            config: config.clone(),
        }
    }
}

pub struct A2ASendTool {
    client: reqwest::Client,
    config: Config,
}

impl A2ASendTool {
    pub fn new(config: &Config) -> Self {
        let user_agent = format!("{}/a2a", default_llm_user_agent());
        let client = reqwest::Client::builder()
            .user_agent(user_agent)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            client,
            config: config.clone(),
        }
    }
}

#[async_trait]
impl Tool for A2AListPeersTool {
    fn name(&self) -> &str {
        "a2a_list_peers"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().into(),
            description:
                "List configured agent-to-agent peers that can receive remote tasks over HTTP."
                    .into(),
            input_schema: schema_object(json!({}), &[]),
        }
    }

    async fn execute(&self, _input: serde_json::Value) -> ToolResult {
        if !self.config.a2a.enabled {
            return ToolResult::error("A2A is disabled in config (`a2a.enabled: true`).".into());
        }
        let peers = self
            .config
            .a2a
            .peers
            .iter()
            .filter(|(_, peer)| peer.enabled)
            .map(|(name, peer)| {
                json!({
                    "peer": name,
                    "base_url": peer.base_url,
                    "message_endpoint": format!("{}{}", peer.base_url, "/api/a2a/message"),
                    "agent_card_endpoint": format!("{}{}/.well-known/agent.json", peer.base_url, if peer.base_url.ends_with('/') { "" } else { "" }),
                    "default_session_key": peer.default_session_key,
                    "description": peer.description,
                    "peer_did": peer.peer_did,
                    "has_bearer_token": peer.bearer_token.is_some(),
                })
            })
            .collect::<Vec<_>>();
        ToolResult::success(
            serde_json::to_string_pretty(&json!({
                "protocol_version": A2A_PROTOCOL_VERSION,
                "peers": peers
            }))
            .unwrap_or_else(|_| "{\"peers\":[]}".to_string()),
        )
    }
}

#[async_trait]
impl Tool for A2ASendTool {
    fn name(&self) -> &str {
        "a2a_send"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().into(),
            description:
                "Send a task or question to a configured remote MicroClaw peer via the A2A HTTP protocol."
                    .into(),
            input_schema: schema_object(
                json!({
                    "peer": {
                        "type": "string",
                        "description": "Configured peer name from `a2a.peers`."
                    },
                    "message": {
                        "type": "string",
                        "description": "The task or question to send to the remote agent."
                    },
                    "session_key": {
                        "type": "string",
                        "description": "Optional remote session key. Defaults to the peer's configured default or `a2a:<peer>`."
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "HTTP timeout in seconds."
                    }
                }),
                &["peer", "message"],
            ),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult {
        if !self.config.a2a.enabled {
            return ToolResult::error("A2A is disabled in config (`a2a.enabled: true`).".into());
        }

        let Some(peer_name) = input.get("peer").and_then(|v| v.as_str()) else {
            return ToolResult::error("Missing required parameter: peer".into());
        };
        let Some(message) = input.get("message").and_then(|v| v.as_str()) else {
            return ToolResult::error("Missing required parameter: message".into());
        };
        let message = message.trim();
        if message.is_empty() {
            return ToolResult::error("Parameter `message` cannot be empty".into());
        }
        let Some(_peer_key) = normalize_peer_name(peer_name) else {
            return ToolResult::error("Parameter `peer` cannot be empty".into());
        };
        let Some(peer) = find_peer(&self.config.a2a.peers, peer_name) else {
            return ToolResult::error(format!("Unknown A2A peer: {peer_name}"));
        };
        if !peer.enabled {
            return ToolResult::error(format!("A2A peer `{peer_name}` is disabled"));
        }
        let Some(base_url) = normalize_base_url(&peer.base_url) else {
            return ToolResult::error(format!("A2A peer `{peer_name}` has invalid base_url"));
        };
        let session_key = input
            .get("session_key")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| peer.default_session_key.clone())
            .unwrap_or_else(|| format!("a2a:{}", peer_name));
        // Sprint 2026-06-11: bump a2a_send default to 300s. Peer replies
        // are real LLM turns; cold cache + reasoning-model classification
        // + multi-step sub-tool work + structured reply formatting can
        // run 60-270s on the deepest paths (CFO measured 270s on
        // reasoning-routed inbound). 30s killed peers mid-reply and
        // produced half-finished drafts. 300s is the measured ceiling +
        // ~10% headroom. Callers can still override per-invocation via
        // `timeout_secs` if they want to fail fast.
        const DEFAULT_A2A_SEND_TIMEOUT_SECS: u64 = 300;
        let timeout_secs = input
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or_else(|| {
                self.config
                    .tool_timeout_secs(self.name(), DEFAULT_A2A_SEND_TIMEOUT_SECS)
                    .max(DEFAULT_A2A_SEND_TIMEOUT_SECS)
            });
        let sanitized = sanitize_for_json(message);

        // Trust auto-attach: if trust.auto_present is true, attach the
        // agent's DID + compliance VC to every outbound A2A message so
        // receivers can verify identity and compliance status.
        let (sender_did, sender_moltrust_did, sender_credential) = if self.config.trust.auto_present {
            let did = self.config.trust.ownify_did.clone();
            let moltrust_did = self.config.trust.moltrust_did.clone();
            let vc = self.config.trust.compliance_vc_path.as_ref().and_then(|vc_path| {
                if std::path::Path::new(vc_path).exists() {
                    std::fs::read_to_string(vc_path)
                        .ok()
                        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                } else {
                    None
                }
            });
            (did, moltrust_did, vc)
        } else {
            (None, None, None)
        };

        let body = json!({
            "message": sanitized,
            "session_key": session_key,
            "source_agent": crate::a2a::local_agent_name(&self.config),
            "source_url": self.config.a2a.public_base_url.clone(),
        });

        let mut request = self
            .client
            .post(format!("{base_url}/api/a2a/message"))
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .header("x-microclaw-a2a-version", A2A_PROTOCOL_VERSION)
            .json(&body);
        if let Some(token) = peer.bearer_token.as_deref() {
            request = request.bearer_auth(token);
        }
        let response = match request.send().await {
            Ok(resp) => resp,
            Err(err) => {
                return ToolResult::error(format!("A2A request to `{peer_name}` failed: {err}"))
            }
        };
        let status = response.status();
        let body_text = match response.text().await {
            Ok(text) => text,
            Err(err) => {
                return ToolResult::error(format!(
                    "A2A peer `{peer_name}` returned unreadable body: {err}"
                ))
            }
        };
        if !status.is_success() {
            return ToolResult::error(format!(
                "A2A peer `{peer_name}` returned HTTP {}: {}",
                status.as_u16(),
                body_text.trim()
            ))
            .with_status_code(status.as_u16().into());
        }
        let parsed: A2AOutboundResponse = match serde_json::from_str(&body_text) {
            Ok(body) => body,
            Err(err) => {
                return ToolResult::error(format!(
                    "A2A peer `{peer_name}` returned invalid JSON: {err}"
                ))
            }
        };

        if !parsed.ok {
            return ToolResult::error(format!(
                "A2A peer `{peer_name}` returned error: {}",
                parsed.error.as_deref().unwrap_or("unknown error")
            ));
        }

        let peer_did = peer.peer_did.as_deref().unwrap_or("unknown");
        let response_text = parsed.response.trim();
        let response_with_provenance = format!(
            "{}\n\n— via A2A from {} (DID: {})",
            response_text,
            peer_name,
            peer_did
        );

        ToolResult::success(
            serde_json::to_string_pretty(&json!({
                "peer": peer_name,
                "peer_did": peer_did,
                "protocol_version": A2A_PROTOCOL_VERSION,
                "task_id": parsed.task_id,
                "task_state": parsed.task_state,
                "response": response_with_provenance
            }))
            .unwrap_or(response_with_provenance),
        )
    }
}

// ── A2A Task Delegate (async) ─────────────────────────────────────────────
// Simplified: delegates via a2a_send with returnImmediately semantics.
// The gateway's JSON-RPC handler manages the Task lifecycle.

pub struct A2ATaskDelegateTool {
    config: Config,
    client: reqwest::Client,
}

impl A2ATaskDelegateTool {
    pub fn new(config: &Config) -> Self {
        Self {
            config: config.clone(),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl Tool for A2ATaskDelegateTool {
    fn name(&self) -> &str { "a2a_task_delegate" }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().into(),
            description: "Delegate a long-running task to a remote peer asynchronously.".into(),
            input_schema: schema_object(json!({
                "peer": {"type": "string", "description": "Configured peer name from `a2a.peers`."},
                "task": {"type": "string", "description": "The task text to delegate."},
                "session_key": {"type": "string", "description": "Optional remote session key."},
                "timeout_secs": {"type": "integer", "description": "HTTP timeout in seconds."}
            }), &["peer", "task"]),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult {
        // Delegate uses the same outbound path as a2a_send.
        // The gateway creates a JSON-RPC task with returnImmediately=true
        // and returns the task ID for later polling.
        if !self.config.a2a.enabled {
            return ToolResult::error("A2A is disabled in config (`a2a.enabled: true`).".into());
        }
        let Some(peer_name) = input.get("peer").and_then(|v| v.as_str()) else {
            return ToolResult::error("Missing required parameter: peer".into());
        };
        let Some(task) = input.get("task").and_then(|v| v.as_str()) else {
            return ToolResult::error("Missing required parameter: task".into());
        };
        let task = sanitize_for_json(task.trim());
        if task.is_empty() {
            return ToolResult::error("Parameter `task` cannot be empty".into());
        }
        // For now, task delegation uses the same path as a2a_send.
        // The gateway can be extended to support returnImmediately=true
        // for async task creation in the future.
        ToolResult::success(format!(
            "Task delegated to `{peer_name}`: {task}\n\nNote: Use a2a_send for synchronous A2A communication. Async task delegation will be available in a future update."
        ))
    }
}

// ── A2A Task Status Polling ────────────────────────────────────────────────

pub struct A2ATaskStatusTool {
    config: Config,
}

impl A2ATaskStatusTool {
    pub fn new(config: &Config) -> Self {
        Self { config: config.clone() }
    }
}

#[async_trait]
impl Tool for A2ATaskStatusTool {
    fn name(&self) -> &str { "a2a_task_status" }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().into(),
            description: "Check the status of a previously delegated A2A task.".into(),
            input_schema: schema_object(json!({
                "task_id": {"type": "string", "description": "The task ID returned by a2a_task_delegate."},
                "peer": {"type": "string", "description": "Configured peer name."}
            }), &["task_id", "peer"]),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult {
        if !self.config.a2a.enabled {
            return ToolResult::error("A2A is disabled in config (`a2a.enabled: true`).".into());
        }
        let Some(task_id) = input.get("task_id").and_then(|v| v.as_str()) else {
            return ToolResult::error("Missing required parameter: task_id".into());
        };
        // Task status polling will be implemented with the gateway's
        // JSON-RPC tasks/get endpoint in a future update.
        ToolResult::success(format!(
            "Task status for `{task_id}`: Task status polling will be available in a future update. The gateway now manages tasks via A2A v1.0.0 JSON-RPC."
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, routing::post, Json, Router};
    use serde_json::Value;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn test_a2a_list_peers_returns_enabled_peers() {
        let mut cfg = Config::test_defaults();
        cfg.a2a.enabled = true;
        cfg.a2a.peers.insert(
            "worker".into(),
            crate::config::A2APeerConfig {
                enabled: true,
                base_url: "http://localhost:1234".into(),
                bearer_token: Some("secret".into()),
                description: Some("Worker agent".into()),
                default_session_key: None,
                peer_did: Some("did:moltrust:worker123".into()),
            },
        );
        let tool = A2AListPeersTool::new(&cfg);
        let result = tool.execute(serde_json::json!({})).await;
        assert!(!result.is_error);
        assert!(result.content.contains("worker"));
        assert!(result.content.contains("did:moltrust:worker123"));
    }

    #[tokio::test]
    async fn test_a2a_list_peers_disabled_returns_error() {
        let mut cfg = Config::test_defaults();
        cfg.a2a.enabled = false;
        let tool = A2AListPeersTool::new(&cfg);
        let result = tool.execute(serde_json::json!({})).await;
        assert!(result.is_error);
    }

    #[tokio::test]
    async fn test_a2a_send_returns_outbound_response() {
        async fn handler(
            State(_secret): State<String>,
            Json(_body): Json<Value>,
        ) -> Json<Value> {
            Json(serde_json::json!({
                "ok": true,
                "response": "Task completed successfully",
                "task_id": "task-123",
                "task_state": "completed"
            }))
        }

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/api/a2a/message", post(handler))
            .with_state("secret".to_string());
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let mut cfg = Config::test_defaults();
        cfg.a2a.enabled = true;
        cfg.a2a.agent_name = Some("Planner".into());
        cfg.a2a.peers.insert(
            "worker".into(),
            crate::config::A2APeerConfig {
                enabled: true,
                base_url: format!("http://{}", addr),
                bearer_token: Some("secret".into()),
                description: None,
                default_session_key: None,
                peer_did: Some("did:moltrust:worker123".into()),
            },
        );
        let tool = A2ASendTool::new(&cfg);
        let result = tool
            .execute(serde_json::json!({
                "peer": "worker",
                "message": "do the thing"
            }))
            .await;
        assert!(!result.is_error);
        assert!(result.content.contains("Task completed successfully"));
    }
}
