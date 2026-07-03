use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

use super::{schema_object, Tool, ToolResult};
use crate::config::Config;
use crate::trust::sha256_hex;
use microclaw_core::llm_types::ToolDefinition;
use microclaw_storage::db::{call_blocking, Database};

pub struct LogComplianceActionTool {
    config: Config,
    db: Arc<Database>,
}

impl LogComplianceActionTool {
    pub fn new(config: &Config, db: Arc<Database>) -> Self {
        Self { config: config.clone(), db }
    }
}

#[async_trait]
impl Tool for LogComplianceActionTool {
    fn name(&self) -> &str {
        "log_compliance_action"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().into(),
            description: "Log a compliance-relevant action to the hash-chained audit trail \
                          (EU AI Act Article 12). Each entry is linked to the previous one \
                          via a SHA-256 hash chain, making the audit trail tamper-evident."
                .into(),
            input_schema: schema_object(
                json!({
                    "action": {
                        "type": "string",
                        "description": "Description of the action being logged \
                                        (e.g. 'web_fetch', 'a2a_send', 'memory_write')."
                    },
                    "article": {
                        "type": "string",
                        "description": "EU AI Act article this action relates to \
                                        (e.g. '10', '11', '12', '43', 'annex-v')."
                    },
                    "payload": {
                        "type": "string",
                        "description": "JSON string of the action payload (will be hashed)."
                    }
                }),
                &["action"],
            ),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult {
        if !self.config.trust.log_compliance_actions {
            return ToolResult::success(
                "{\"skipped\": true, \"reason\": \"log_compliance_actions is false\"}".into()
            );
        }

        let action = input.get("action").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
        let article = input.get("article").and_then(|v| v.as_str()).unwrap_or("12").to_string();
        let payload = input.get("payload").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let agent_did = self.config.trust.ownify_did.clone();
        let timestamp = chrono::Utc::now().to_rfc3339();
        let payload_hash = sha256_hex(payload.as_bytes());

        let db = self.db.clone();

        let ensure_result = call_blocking(db.clone(), |db| db.ensure_compliance_audit_table()).await;
        if let Err(e) = ensure_result {
            return ToolResult::error(format!("Failed to create compliance_audit table: {}", e));
        }

        let prev_hash = match call_blocking(db.clone(), |db| db.get_last_compliance_hash()).await {
            Ok(h) => h,
            Err(e) => return ToolResult::error(format!("Failed to get previous hash: {}", e)),
        };

        let action_for_db = action.clone();
        let article_for_db = article.clone();
        let payload_hash_for_db = payload_hash.clone();
        let prev_hash_for_db = prev_hash.clone();
        let timestamp_for_db = timestamp.clone();
        let agent_did_for_db = agent_did.clone();

        let seq = match call_blocking(db, move |db| {
            db.insert_compliance_audit(
                &action_for_db,
                &payload_hash_for_db,
                &prev_hash_for_db,
                &timestamp_for_db,
                &article_for_db,
                agent_did_for_db.as_deref(),
            )
        }).await {
            Ok(seq) => seq,
            Err(e) => return ToolResult::error(format!("Failed to insert audit entry: {}", e)),
        };

        let result = json!({
            "seq": seq,
            "action": action,
            "article": article,
            "payload_hash": payload_hash,
            "prev_hash": prev_hash,
            "timestamp": timestamp,
            "agent_did": agent_did,
        });

        ToolResult::success(serde_json::to_string_pretty(&result).unwrap_or_default())
    }
}