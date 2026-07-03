use async_trait::async_trait;
use serde_json::json;
use std::path::Path;

use super::{schema_object, Tool, ToolResult};
use crate::config::Config;
use crate::skill_verifier::{compute_skill_hash, verify_skill_content, SkillVerdict};
use microclaw_core::llm_types::ToolDefinition;

pub struct VerifySkillTool {
    config: Config,
}

impl VerifySkillTool {
    pub fn new(config: &Config) -> Self {
        Self { config: config.clone() }
    }
}

#[async_trait]
impl Tool for VerifySkillTool {
    fn name(&self) -> &str {
        "verify_skill"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().into(),
            description: "Verify a skill against the ownify-skill-registry. \
                          Hashes the skill content and checks the registry for \
                          ATR + MolTrust audit results. Returns verification \
                          status, score, and findings."
                .into(),
            input_schema: schema_object(
                json!({
                    "skill_name": {
                        "type": "string",
                        "description": "Name of the skill to verify (as it appears in the skills directory)."
                    }
                }),
                &["skill_name"],
            ),
        }
    }

    async fn execute(&self, input: serde_json::Value) -> ToolResult {
        let skill_name = match input.get("skill_name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => return ToolResult::error("Missing required parameter: skill_name".into()),
        };

        let registry_url = match &self.config.trust.skill_registry_url {
            Some(url) => url.clone(),
            None => return ToolResult::error(
                "No skill registry URL configured. Set trust.skill_registry_url in config.".into()
            ),
        };

        let skills_dir = self.config.skills_data_dir();
        let skill_dir = Path::new(&skills_dir).join(skill_name);
        let skill_md_path = skill_dir.join("SKILL.md");

        if !skill_md_path.exists() {
            return ToolResult::error(format!(
                "Skill '{}' not found at {:?}", skill_name, skill_md_path
            ));
        }

        let content = match std::fs::read_to_string(&skill_md_path) {
            Ok(c) => c,
            Err(e) => return ToolResult::error(format!("Failed to read SKILL.md: {}", e)),
        };

        let hash = compute_skill_hash(&content);
        let result = verify_skill_content(&content, &registry_url).await;

        let verdict_str = match result.verdict {
            SkillVerdict::Verified => "verified",
            SkillVerdict::Audited => "audited",
            SkillVerdict::AtrBlocked => "atr_blocked",
            SkillVerdict::MtFailed => "mt_failed",
            SkillVerdict::Expired => "expired",
            SkillVerdict::NotFound => "not_found",
            SkillVerdict::RegistryUnreachable => "registry_unreachable",
        };

        let output = json!({
            "skill_name": skill_name,
            "hash": hash,
            "verdict": verdict_str,
            "score": result.score,
            "findings": result.findings,
            "registry_url": registry_url,
        });

        ToolResult::success(serde_json::to_string_pretty(&output).unwrap_or_default())
    }
}