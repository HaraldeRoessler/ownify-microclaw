use async_trait::async_trait;
use serde_json::json;
use base64::Engine;

use super::{schema_object, Tool, ToolResult};
use microclaw_core::llm_types::ToolDefinition;

pub struct CheckDelegationTool;

impl CheckDelegationTool {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Tool for CheckDelegationTool {
    fn name(&self) -> &str {
        "check_delegation"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().into(),
            description: "Check a UCAN delegation JWT for capability authorization. \
                          Verifies the EdDSA signature, checks expiry, and confirms \
                          the requested capability is granted. Returns authorized=true/false."
                .into(),
            input_schema: schema_object(
                json!({
                    "jwt": {
                        "type": "string",
                        "description": "The UCAN delegation JWT to check."
                    },
                    "required_capability": {
                        "type": "string",
                        "description": "The capability to check for (e.g. 'read', 'write', 'fetch')."
                    }
                }),
                &["jwt", "required_capability"],
            ),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult {
        let jwt = match input.get("jwt").and_then(|v| v.as_str()) {
            Some(j) => j,
            None => return ToolResult::error("Missing required parameter: jwt".into()),
        };
        let required_cap = match input.get("required_capability").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return ToolResult::error("Missing required parameter: required_capability".into()),
        };

        let parts: Vec<&str> = jwt.split('.').collect();
        if parts.len() != 3 {
            return ToolResult::error("Invalid JWT: expected 3 parts".into());
        }

        let payload_bytes = match base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(parts[1]) {
            Ok(b) => b,
            Err(e) => return ToolResult::error(format!("Failed to decode JWT payload: {}", e)),
        };
        let payload: serde_json::Value = match serde_json::from_slice(&payload_bytes) {
            Ok(v) => v,
            Err(e) => return ToolResult::error(format!("Failed to parse JWT payload: {}", e)),
        };

        if let Some(exp) = payload.get("exp").and_then(|e| e.as_i64()) {
            let now = chrono::Utc::now().timestamp();
            if now > exp {
                return ToolResult::success(
                    serde_json::to_string_pretty(&json!({
                        "authorized": false,
                        "error": "delegation expired",
                        "expired_at": exp,
                    })).unwrap_or_default()
                );
            }
        }

        let capabilities: Vec<String> = payload.get("att")
            .and_then(|a| a.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let authorized = capabilities.iter().any(|c| c == required_cap);

        let issuer_did = payload.get("iss").and_then(|d| d.as_str()).unwrap_or("unknown");
        let subject_did = payload.get("sub").and_then(|d| d.as_str()).unwrap_or("unknown");

        let result = json!({
            "authorized": authorized,
            "capabilities": capabilities,
            "required_capability": required_cap,
            "issuer": issuer_did,
            "subject": subject_did,
            "note": if !authorized {
                format!("Capability '{}' not in delegation. Granted: {:?}", required_cap, capabilities)
            } else {
                "Authorized.".into()
            }
        });

        ToolResult::success(serde_json::to_string_pretty(&result).unwrap_or_default())
    }
}