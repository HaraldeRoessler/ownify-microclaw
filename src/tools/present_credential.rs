use async_trait::async_trait;
use serde_json::json;
use std::path::Path;

use super::{schema_object, Tool, ToolResult};
use crate::config::Config;
use microclaw_core::llm_types::ToolDefinition;

pub struct PresentCredentialTool {
    config: Config,
}

impl PresentCredentialTool {
    pub fn new(config: &Config) -> Self {
        Self { config: config.clone() }
    }
}

#[async_trait]
impl Tool for PresentCredentialTool {
    fn name(&self) -> &str {
        "present_credential"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().into(),
            description: "Present this agent's DID and compliance Verifiable Credential \
                          for inclusion in A2A outbound messages. Returns the DID and VC \
                          JSON that should be attached to outbound communications so \
                          receivers can verify the agent's identity and compliance status."
                .into(),
            input_schema: schema_object(json!({}), &[]),
        }
    }

    async fn execute(&self, _input: serde_json::Value) -> ToolResult {
        let trust = &self.config.trust;

        let did = match &trust.ownify_did {
            Some(d) => d.clone(),
            None => return ToolResult::error(
                "No ownify DID configured. Set trust.ownify_did in config or \
                 generate a compliance VC in the ownify portal first.".into()
            ),
        };

        let moltrust_did = trust.moltrust_did.clone();

        let vc = match &trust.compliance_vc_path {
            Some(path) if Path::new(path).exists() => {
                match std::fs::read_to_string(path) {
                    Ok(content) => {
                        match serde_json::from_str::<serde_json::Value>(&content) {
                            Ok(vc_json) => Some(vc_json),
                            Err(e) => return ToolResult::error(
                                format!("Failed to parse compliance VC at {}: {}", path, e)
                            ),
                        }
                    }
                    Err(e) => return ToolResult::error(
                        format!("Failed to read compliance VC at {}: {}", path, e)
                    ),
                }
            }
            Some(path) => return ToolResult::error(
                format!("Compliance VC file not found at path: {}", path)
            ),
            None => None,
        };

        let result = json!({
            "did": did,
            "moltrust_did": moltrust_did,
            "compliance_vc": vc,
            "note": if vc.is_none() {
                "No compliance VC loaded — only DID will be presented. \
                 Generate an Annex V declaration in the ownify portal to add compliance proof."
            } else {
                "DID + compliance VC ready for A2A outbound attachment."
            }
        });

        ToolResult::success(serde_json::to_string_pretty(&result).unwrap_or_default())
    }
}