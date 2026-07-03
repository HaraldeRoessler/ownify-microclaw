use async_trait::async_trait;
use serde_json::json;

use super::{schema_object, Tool, ToolResult};
use crate::trust::{fetch_did_document, verify_vc};
use microclaw_core::llm_types::ToolDefinition;

pub struct VerifyCredentialTool;

impl VerifyCredentialTool {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Tool for VerifyCredentialTool {
    fn name(&self) -> &str {
        "verify_credential"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().into(),
            description: "Verify a W3C Verifiable Credential locally. \
                          Checks the Ed25519 signature against the issuer's DID document, \
                          verifies expiry, and detects tampering. No API call needed \
                          for did:web issuers."
                .into(),
            input_schema: schema_object(
                json!({
                    "vc": {
                        "type": "object",
                        "description": "The Verifiable Credential JSON to verify."
                    },
                    "did_document": {
                        "type": "object",
                        "description": "Optional: the issuer's DID document. \
                                        If omitted, resolves did:web automatically."
                    }
                }),
                &["vc"],
            ),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult {
        let vc = match input.get("vc") {
            Some(v) => v.clone(),
            None => return ToolResult::error("Missing required parameter: vc".into()),
        };

        let issuer_did = match vc.get("issuer").and_then(|d| d.as_str()) {
            Some(d) => d.to_string(),
            None => return ToolResult::error("VC missing issuer field".into()),
        };

        let did_doc = if let Some(doc) = input.get("did_document") {
            doc.clone()
        } else if issuer_did.starts_with("did:web:") {
            match fetch_did_document(&issuer_did).await {
                Ok(doc) => doc,
                Err(e) => return ToolResult::error(
                    format!("Failed to resolve DID document for {}: {}", issuer_did, e)
                ),
            }
        } else {
            return ToolResult::error(
                format!("Cannot resolve DID document for {} — only did:web is supported. \
                         Provide did_document in the request.", issuer_did)
            );
        };

        match verify_vc(&vc, &did_doc) {
            Ok(result) => {
                let output = json!({
                    "verified": result.verified,
                    "issuer": issuer_did,
                    "error": result.error,
                });
                ToolResult::success(serde_json::to_string_pretty(&output).unwrap_or_default())
            }
            Err(e) => ToolResult::error(format!("Verification error: {}", e)),
        }
    }
}