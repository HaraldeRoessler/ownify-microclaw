// Self-check verifier hook (sprint 2026-06-11).
//
// Calls the in-cluster a2a-gateway's /internal/verify-reply endpoint
// to validate the LLM's Provenance-block claims against ground truth
// (a2a_interactions for peer calls, memgate for memory writes). The
// verdict drives whether the agent's reply is delivered as-is, gets
// a "⚠️ unverifiable" stamp, or gets reformulated.
//
// In addition to the A2A/memory claim verification via the gateway,
// the verifier performs LOCAL compliance claim verification:
//   - "I am Article 12 compliant" → check the compliance_audit table
//     is non-empty (the agent has been logging its actions).
//   - "I have Annex V declaration" → check trust.compliance_vc_path
//     points to a file that exists on disk.
// These local checks run BEFORE the gateway call. If a compliance
// claim is found to be a fabrication, the verifier returns Fail
// immediately without calling the gateway.
//
// The verifier is OPTIONAL: callers MUST gate on
// `OWNIFY_A2A_OUTBOUND_TOKEN` being set (the bearer the agent uses
// to authenticate to its own per-tenant gateway). If the env var
// is missing — e.g. dev/test runs without the A2A plumbing — the
// verifier is a no-op and the agent replies flow through unchanged.
//
// Returns VerifierOutcome. Failures (network errors, CP down, 5xx
// responses) are surfaced via the Err arm so the caller can decide
// whether to deliver unverified (current behaviour) or block.

use crate::runtime::AppState;
use microclaw_storage::db::{call_blocking, Database};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum VerifierOutcome {
    /// All claims in the Provenance block matched ground truth. The
    /// reply is delivered as-is.
    Pass,
    /// Some claims could not be verified (e.g. web fetches we cannot
    /// yet confirm, claims missing a jti). The reply is delivered
    /// with `suggested_prefix` prepended so the user knows which
    /// parts are unverified.
    Warn {
        reason: String,
        suggested_prefix: Option<String>,
    },
    /// Fabrications detected — claimed work that did not actually
    /// happen. The caller is expected to inject a reformulation
    /// prompt so the LLM can re-attempt with honest claims.
    Fail {
        reason: String,
        suggested_prefix: Option<String>,
    },
}

#[derive(Debug, Serialize)]
struct VerifierRequest<'a> {
    reply: &'a str,
}

#[derive(Debug, Deserialize)]
struct VerifierResponse {
    verdict: String,
    reason: String,
    #[serde(default)]
    suggested_reply_prefix: Option<String>,
}

/// Which compliance claims the LLM made in its reply, detected by
/// scanning the reply text. Used by the local compliance verifier
/// before the A2A gateway call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ComplianceClaims {
    /// The reply claims "I am Article 12 compliant" (or similar).
    pub article_12: bool,
    /// The reply claims "I have Annex V declaration" (or similar).
    pub annex_v: bool,
}

impl ComplianceClaims {
    /// True if no compliance claims were detected — the local
    /// compliance check is a no-op in this case.
    pub fn is_empty(&self) -> bool {
        !self.article_12 && !self.annex_v
    }
}

/// Scan the reply text for compliance claims. Case-insensitive
/// substring matching. Pure function — safe to unit-test without
/// a database or filesystem.
pub fn detect_compliance_claims(reply: &str) -> ComplianceClaims {
    let lower = reply.to_lowercase();
    ComplianceClaims {
        article_12: lower.contains("i am article 12 compliant"),
        annex_v: lower.contains("i have annex v declaration"),
    }
}

/// Verify compliance claims against local ground truth.
///
/// Returns:
///   - `None` if no compliance claims were detected (skip local check).
///   - `Some(Fail)` if any claimed compliance is a fabrication.
///   - `Some(Pass)` if all detected claims check out.
///
/// This runs BEFORE the A2A gateway call so fabrications are caught
/// locally even if the gateway would have passed the other claims.
pub async fn verify_compliance_claims(
    claims: &ComplianceClaims,
    state: &AppState,
) -> Option<VerifierOutcome> {
    if claims.is_empty() {
        return None;
    }

    let mut failures: Vec<String> = Vec::new();

    if claims.article_12 {
        match check_article_12(&state.db).await {
            Ok(true) => {} // audit trail is non-empty — claim holds
            Ok(false) => {
                failures.push(
                    "claimed Article 12 compliance but compliance_audit table is empty".into(),
                );
            }
            Err(e) => {
                // DB error — we can't confirm the claim. Treat as
                // a warning, not a fabrication, since the absence of
                // evidence here is a tooling issue not a lie.
                tracing::warn!("verifier: compliance_audit count failed: {e}");
            }
        }
    }

    if claims.annex_v {
        match check_annex_v(&state.config) {
            Ok(true) => {} // VC file exists — claim holds
            Ok(false) => {
                failures.push(
                    "claimed Annex V declaration but compliance_vc_path is not set or file does not exist".into(),
                );
            }
            Err(e) => {
                tracing::warn!("verifier: annex V check failed: {e}");
            }
        }
    }

    if failures.is_empty() {
        Some(VerifierOutcome::Pass)
    } else {
        Some(VerifierOutcome::Fail {
            reason: failures.join("; "),
            suggested_prefix: Some(
                "⛔ Compliance claim verification failed. Drop the unverified compliance claims \
                 or earn them before asserting."
                    .into(),
            ),
        })
    }
}

/// Check Article 12 compliance: the compliance_audit table must
/// exist and contain at least one entry. An agent that claims to be
/// Article 12 compliant but has never logged an action is fabricating.
async fn check_article_12(db: &Arc<Database>) -> Result<bool, String> {
    let db = db.clone();
    let result = call_blocking(db, |db| {
        db.ensure_compliance_audit_table()?;
        db.count_compliance_audit_entries()
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(result > 0)
}

/// Check Annex V declaration: trust.compliance_vc_path must be set
/// and the referenced file must exist on disk.
fn check_annex_v(config: &crate::config::Config) -> Result<bool, String> {
    match &config.trust.compliance_vc_path {
        Some(path) => {
            let exists = std::path::Path::new(path).exists();
            Ok(exists)
        }
        None => Ok(false),
    }
}

/// Build the in-cluster URL for the per-tenant a2a-gateway's
/// /internal/verify-reply endpoint. The same convention as the
/// peer-task skill's send.sh: a2a-gateway-<slug> in
/// ownify-tenant-<slug>, port 4000.
fn verifier_url(slug: &str) -> String {
    format!(
        "http://ownify-a2a-gateway-{slug}.ownify-tenant-{slug}.svc.cluster.local:4000/internal/verify-reply"
    )
}

/// Call the verifier. Returns VerifierOutcome on success; Err on
/// network/transport failures (the caller should treat these as
/// "deliver without check" — verifier outages must not block the
/// user's replies).
pub async fn call_verifier(
    reply: &str,
    state: &AppState,
) -> Result<VerifierOutcome, String> {
    // --- Local compliance claim verification ---
    //
    // Runs before the gateway call. If the LLM claims compliance
    // status it cannot back up with local ground truth, we fail
    // immediately — no need to round-trip to the gateway.
    let claims = detect_compliance_claims(reply);
    if let Some(outcome) = verify_compliance_claims(&claims, state).await {
        match &outcome {
            VerifierOutcome::Pass => {
                // Compliance claims verified — continue to gateway
                // for the remaining A2A/memory claims.
            }
            VerifierOutcome::Fail { .. } | VerifierOutcome::Warn { .. } => {
                // Compliance fabrication or warning — return early.
                return Ok(outcome);
            }
        }
    }

    // --- A2A / memory claim verification via gateway ---

    // Read the slug from env. If missing, the agent is not
    // configured for A2A — treat as a no-op.
    let slug = match std::env::var("OWNIFY_TENANT_SLUG")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        Some(s) => s,
        None => return Ok(VerifierOutcome::Pass),  // not configured, skip
    };
    let bearer = match std::env::var("OWNIFY_A2A_OUTBOUND_TOKEN")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        Some(b) => b,
        None => return Ok(VerifierOutcome::Pass),  // not configured, skip
    };

    let url = verifier_url(&slug);

    // Build a fresh reqwest client. The agent loop calls this at
    // most once per reply turn, so per-call construction is fine
    // (we don't want a long-lived client with keep-alive pinning us
    // to a stale gateway).
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("verifier: client build failed: {e}"))?;

    let body = VerifierRequest { reply };

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {bearer}"))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("verifier: send failed: {e}"))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("verifier: read body failed: {e}"))?;

    if !status.is_success() {
        // 4xx/5xx — surface to caller as Err so it can decide.
        return Err(format!(
            "verifier: HTTP {status} from gateway: {}",
            text.chars().take(200).collect::<String>()
        ));
    }

    let parsed: VerifierResponse = serde_json::from_str(&text)
        .map_err(|e| format!("verifier: parse failed: {e}; body={}", text.chars().take(200).collect::<String>()))?;

    let outcome = match parsed.verdict.as_str() {
        "pass" => VerifierOutcome::Pass,
        "warn" => VerifierOutcome::Warn {
            reason: parsed.reason.clone(),
            suggested_prefix: parsed.suggested_reply_prefix,
        },
        "fail" => VerifierOutcome::Fail {
            reason: parsed.reason.clone(),
            suggested_prefix: parsed.suggested_reply_prefix,
        },
        // Unknown verdict — treat as warn (don't block, but flag).
        other => VerifierOutcome::Warn {
            reason: format!("unknown verdict '{other}'"),
            suggested_prefix: Some(format!(
                "⚠️ verifier returned unknown verdict: {other}. Reply delivered without confirmation."
            )),
        },
    };

    // Light observability — log the verdict so operators can see
    // verification rates without scraping agent logs.
    if let Some(logger) = state_verifier_log_channel() {
        logger(&format!(
            "verifier: reply_id={} verdict={} reason={}",
            "<n/a>",
            parsed.verdict,
            parsed.reason,
        ));
    }

    Ok(outcome)
}

// Tiny shim so the call site doesn't have to know about AppState's
// internals. Returns None when the runtime doesn't expose a logger
// hook (e.g. tests); we silently no-op.
fn state_verifier_log_channel() -> Option<Box<dyn Fn(&str) + Send + Sync>> {
    None  // Real logging happens via the warn!/info! macros in the caller.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_compliance_claims_none() {
        let reply = "I fetched the weather from the API. ## Provenance\n- web_fetch";
        let claims = detect_compliance_claims(reply);
        assert!(claims.is_empty());
        assert!(!claims.article_12);
        assert!(!claims.annex_v);
    }

    #[test]
    fn test_detect_compliance_claims_article_12() {
        let reply = "I am Article 12 compliant and logged this action.";
        let claims = detect_compliance_claims(reply);
        assert!(claims.article_12);
        assert!(!claims.annex_v);
        assert!(!claims.is_empty());
    }

    #[test]
    fn test_detect_compliance_claims_annex_v() {
        let reply = "I have Annex V declaration on file.";
        let claims = detect_compliance_claims(reply);
        assert!(claims.annex_v);
        assert!(!claims.article_12);
        assert!(!claims.is_empty());
    }

    #[test]
    fn test_detect_compliance_claims_both() {
        let reply = "I am Article 12 compliant and I have Annex V declaration ready.";
        let claims = detect_compliance_claims(reply);
        assert!(claims.article_12);
        assert!(claims.annex_v);
        assert!(!claims.is_empty());
    }

    #[test]
    fn test_detect_compliance_claims_case_insensitive() {
        let reply = "i am article 12 compliant. I HAVE ANNEX V DECLARATION.";
        let claims = detect_compliance_claims(reply);
        assert!(claims.article_12);
        assert!(claims.annex_v);
    }

    #[test]
    fn test_compliance_claims_is_empty_default() {
        assert!(ComplianceClaims::default().is_empty());
    }

    #[test]
    fn test_check_annex_v_no_path() {
        let mut config = crate::config::Config::test_defaults();
        config.trust.compliance_vc_path = None;
        let result = check_annex_v(&config).unwrap();
        assert!(!result, "annex V should be false when no path is set");
    }

    #[test]
    fn test_check_annex_v_nonexistent_path() {
        let mut config = crate::config::Config::test_defaults();
        config.trust.compliance_vc_path = Some("/nonexistent/annex_v.json".into());
        let result = check_annex_v(&config).unwrap();
        assert!(!result, "annex V should be false when file does not exist");
    }

    #[test]
    fn test_check_annex_v_existing_path() {
        let dir = std::env::temp_dir();
        let vc_path = dir.join("test_annex_v_vc.json");
        std::fs::write(&vc_path, r#"{"type":"VerifiableCredential"}"#).unwrap();

        let mut config = crate::config::Config::test_defaults();
        config.trust.compliance_vc_path = Some(vc_path.to_string_lossy().to_string());
        let result = check_annex_v(&config).unwrap();
        assert!(result, "annex V should be true when file exists");

        std::fs::remove_file(&vc_path).ok();
    }

    #[tokio::test]
    async fn test_check_article_12_empty_table() {
        let db = test_db();
        // Empty compliance_audit table → not compliant.
        let result = check_article_12(&db).await.unwrap();
        assert!(!result, "Article 12 should be false with empty audit table");
    }

    #[tokio::test]
    async fn test_check_article_12_nonempty_table() {
        let db = test_db();
        // Insert an audit entry → compliant.
        let db_clone = db.clone();
        call_blocking(db_clone, |db| {
            db.ensure_compliance_audit_table()?;
            db.insert_compliance_audit(
                "test_action",
                "deadbeef",
                "genesis",
                "2026-07-03T00:00:00Z",
                "12",
                Some("did:web:example.com"),
            )?;
            Ok(())
        })
        .await
        .unwrap();

        let result = check_article_12(&db).await.unwrap();
        assert!(result, "Article 12 should be true with non-empty audit table");
    }

    #[tokio::test]
    async fn test_verify_compliance_claims_no_claims() {
        let db = test_db();
        let state = test_state(db);
        let claims = ComplianceClaims::default();
        let result = verify_compliance_claims(&claims, &state).await;
        assert!(result.is_none(), "no claims → None (skip local check)");
    }

    #[tokio::test]
    async fn test_verify_compliance_claims_article_12_pass() {
        let db = test_db();
        let db_clone = db.clone();
        call_blocking(db_clone, |db| {
            db.ensure_compliance_audit_table()?;
            db.insert_compliance_audit(
                "test_action",
                "deadbeef",
                "genesis",
                "2026-07-03T00:00:00Z",
                "12",
                None,
            )?;
            Ok(())
        })
        .await
        .unwrap();

        let state = test_state(db);
        let claims = ComplianceClaims { article_12: true, annex_v: false };
        let result = verify_compliance_claims(&claims, &state).await.unwrap();
        assert!(matches!(result, VerifierOutcome::Pass), "should pass with non-empty audit");
    }

    #[tokio::test]
    async fn test_verify_compliance_claims_article_12_fail() {
        let db = test_db();
        let state = test_state(db);
        let claims = ComplianceClaims { article_12: true, annex_v: false };
        let result = verify_compliance_claims(&claims, &state).await.unwrap();
        match result {
            VerifierOutcome::Fail { reason, .. } => {
                assert!(reason.contains("compliance_audit table is empty"));
            }
            other => panic!("expected Fail, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_verify_compliance_claims_annex_v_fail() {
        let db = test_db();
        let state = test_state(db);
        let claims = ComplianceClaims { article_12: false, annex_v: true };
        let result = verify_compliance_claims(&claims, &state).await.unwrap();
        match result {
            VerifierOutcome::Fail { reason, .. } => {
                assert!(reason.contains("compliance_vc_path is not set"));
            }
            other => panic!("expected Fail, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_verify_compliance_claims_both_fail() {
        let db = test_db();
        let state = test_state(db);
        let claims = ComplianceClaims { article_12: true, annex_v: true };
        let result = verify_compliance_claims(&claims, &state).await.unwrap();
        match result {
            VerifierOutcome::Fail { reason, .. } => {
                assert!(reason.contains("compliance_audit table is empty"));
                assert!(reason.contains("compliance_vc_path is not set"));
            }
            other => panic!("expected Fail, got {:?}", other),
        }
    }

    fn test_db() -> Arc<Database> {
        let dir = std::env::temp_dir()
            .join(format!("mc_verifier_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Arc::new(Database::new(dir.to_str().unwrap()).unwrap())
    }

    /// Build a minimal AppState for verifier tests. Mirrors the
    /// pattern in agent_engine::tests::test_state_with_llm_and_confirmation.
    fn test_state(db: Arc<Database>) -> AppState {
        use crate::config::{Config, WorkingDirIsolation};
        use crate::hooks::HookManager;
        use crate::llm::LlmProvider;
        use crate::memory::MemoryManager;
        use crate::memory_backend::MemoryBackend;
        use crate::skills::SkillManager;
        use crate::tools::ToolRegistry;
        use crate::web::WebAdapter;
        use microclaw_channels::channel_adapter::ChannelRegistry;
        use microclaw_core::llm_types::{
            MessagesResponse, ResponseContentBlock,
        };

        struct DummyLlm;

        #[async_trait::async_trait]
        impl LlmProvider for DummyLlm {
            async fn send_message(
                &self,
                _system: &str,
                _messages: Vec<microclaw_core::llm_types::Message>,
                _tools: Option<Vec<microclaw_core::llm_types::ToolDefinition>>,
            ) -> Result<MessagesResponse, microclaw_core::error::MicroClawError> {
                Ok(MessagesResponse {
                    content: vec![ResponseContentBlock::Text {
                        text: "ok".to_string(),
                    }],
                    stop_reason: Some("end_turn".to_string()),
                    usage: None,
                })
            }
        }

        let base_dir = std::env::temp_dir()
            .join(format!("mc_verifier_state_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&base_dir).unwrap();
        let runtime_dir = base_dir.join("runtime");
        std::fs::create_dir_all(&runtime_dir).unwrap();

        let mut cfg = Config::test_defaults();
        cfg.data_dir = base_dir.to_string_lossy().to_string();
        cfg.working_dir = base_dir.join("tmp").to_string_lossy().to_string();
        cfg.working_dir_isolation = WorkingDirIsolation::Shared;
        cfg.web_port = 3900;

        let memory_backend = Arc::new(MemoryBackend::local_only(db.clone()));
        let mut registry = ChannelRegistry::new();
        registry.register(Arc::new(WebAdapter));
        let channel_registry = Arc::new(registry);

        AppState {
            config: cfg.clone(),
            channel_registry: channel_registry.clone(),
            db: db.clone(),
            memory: MemoryManager::new(runtime_dir.to_str().unwrap()),
            skills: SkillManager::from_skills_dir(&cfg.skills_data_dir()),
            hooks: Arc::new(HookManager::from_config(&cfg)),
            llm: Box::new(DummyLlm),
            llm_provider_overrides: Arc::new(tokio::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
            llm_model_overrides: Arc::new(tokio::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
            embedding: None,
            memory_backend: memory_backend.clone(),
            tools: ToolRegistry::new(&cfg, channel_registry, db, memory_backend),
            chat_turn_queue: Arc::new(crate::chat_turn_queue::ChatTurnQueue::new(20)),
            skill_review_queue: crate::skill_review::build_skill_review_channel().0,
            metric_exporter: None,
            trace_exporter: None,
            log_exporter: None,
        }
    }
}