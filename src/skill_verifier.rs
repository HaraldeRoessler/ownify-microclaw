// Skill verification module — checks skills against the ownify-skill-registry.
// At startup, each SKILL.md is hashed and the hash is queried against the
// registry API. Only verified skills are loaded (configurable).
//
// No MolTrust API call — the registry caches verification status in PostgreSQL.

use sha2::{Digest, Sha256};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum SkillVerdict {
    Verified,
    Audited,
    AtrBlocked,
    MtFailed,
    Expired,
    NotFound,
    RegistryUnreachable,
}

#[derive(Debug, Clone)]
pub struct SkillVerificationResult {
    pub verdict: SkillVerdict,
    pub score: Option<i32>,
    pub findings: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct RegistryResponse {
    overall_status: Option<String>,
    atr_score: Option<i32>,
    mt_score: Option<i32>,
    #[allow(dead_code)]
    skill_name: Option<String>,
}

/// Compute SHA-256 hash of skill content, return as hex string.
pub fn compute_skill_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let result = hasher.finalize();
    result.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Verify a skill by its content. Hashes the content and queries the registry.
pub async fn verify_skill_content(
    content: &str,
    registry_url: &str,
) -> SkillVerificationResult {
    let hash = compute_skill_hash(content);
    verify_skill_hash(&hash, registry_url).await
}

/// Verify a skill by its file path. Reads the file and calls verify_skill_content.
pub async fn verify_skill_file(
    path: &Path,
    registry_url: &str,
) -> SkillVerificationResult {
    match std::fs::read_to_string(path) {
        Ok(content) => verify_skill_content(&content, registry_url).await,
        Err(e) => {
            tracing::warn!("Failed to read skill file {:?}: {}", path, e);
            SkillVerificationResult {
                verdict: SkillVerdict::RegistryUnreachable,
                score: None,
                findings: None,
            }
        }
    }
}

/// Query the registry by skill hash.
async fn verify_skill_hash(hash: &str, registry_url: &str) -> SkillVerificationResult {
    let url = format!("{}/api/skills/{}", registry_url.trim_end_matches('/'), hash);

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => return unreachable_result(),
    };

    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("Skill registry unreachable: {}", e);
            return unreachable_result();
        }
    };

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return SkillVerificationResult {
            verdict: SkillVerdict::NotFound,
            score: None,
            findings: None,
        };
    }

    if !resp.status().is_success() {
        tracing::warn!("Skill registry error: HTTP {}", resp.status());
        return unreachable_result();
    }

    let data: RegistryResponse = match resp.json().await {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("Skill registry parse error: {}", e);
            return unreachable_result();
        }
    };

    let status = data.overall_status.unwrap_or_else(|| "not_found".to_string());
    let verdict = match status.as_str() {
        "verified" => SkillVerdict::Verified,
        "audited" => SkillVerdict::Audited,
        "atr_blocked" => SkillVerdict::AtrBlocked,
        "mt_failed" => SkillVerdict::MtFailed,
        "expired" => SkillVerdict::Expired,
        _ => SkillVerdict::NotFound,
    };

    SkillVerificationResult {
        verdict,
        score: data.atr_score.or(data.mt_score),
        findings: None,
    }
}

/// Determine whether a skill should be loaded based on its verification result.
/// - Verified: always load
/// - AtrBlocked / MtFailed: never load
/// - NotFound / Audited / Expired: load only if require_verified is false
/// - RegistryUnreachable: always load (backward compat — don't block on registry outage)
pub fn should_load_skill(result: &SkillVerificationResult, require_verified: bool) -> bool {
    match result.verdict {
        SkillVerdict::Verified => true,
        SkillVerdict::AtrBlocked => false,
        SkillVerdict::MtFailed => false,
        SkillVerdict::RegistryUnreachable => true, // fail-open for backward compat
        SkillVerdict::NotFound => !require_verified,
        SkillVerdict::Audited => !require_verified,
        SkillVerdict::Expired => !require_verified,
    }
}

fn unreachable_result() -> SkillVerificationResult {
    SkillVerificationResult {
        verdict: SkillVerdict::RegistryUnreachable,
        score: None,
        findings: None,
    }
}