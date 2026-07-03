use microclaw::skill_verifier::{compute_skill_hash, should_load_skill, SkillVerificationResult, SkillVerdict};

#[test]
fn test_compute_skill_hash_returns_hex() {
    let content = "# My Skill\nDoes things.";
    let hash = compute_skill_hash(content);
    assert!(hash.len() == 64, "expected 64 hex chars, got {}", hash.len());
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn test_compute_skill_hash_deterministic() {
    let content = "test content";
    let a = compute_skill_hash(content);
    let b = compute_skill_hash(content);
    assert_eq!(a, b);
}

#[test]
fn test_compute_skill_hash_different_inputs() {
    assert_ne!(compute_skill_hash("a"), compute_skill_hash("b"));
}

#[test]
fn test_should_load_verified() {
    let result = SkillVerificationResult {
        verdict: SkillVerdict::Verified,
        score: Some(92),
        findings: None,
    };
    assert!(should_load_skill(&result, true));
    assert!(should_load_skill(&result, false));
}

#[test]
fn test_should_load_atr_blocked() {
    let result = SkillVerificationResult {
        verdict: SkillVerdict::AtrBlocked,
        score: Some(20),
        findings: None,
    };
    assert!(!should_load_skill(&result, true));
    assert!(!should_load_skill(&result, false));
}

#[test]
fn test_should_load_not_found_require_verified() {
    let result = SkillVerificationResult {
        verdict: SkillVerdict::NotFound,
        score: None,
        findings: None,
    };
    assert!(!should_load_skill(&result, true));
    assert!(should_load_skill(&result, false));
}

#[test]
fn test_should_load_audited() {
    let result = SkillVerificationResult {
        verdict: SkillVerdict::Audited,
        score: Some(75),
        findings: None,
    };
    assert!(!should_load_skill(&result, true));
    assert!(should_load_skill(&result, false));
}

#[test]
fn test_should_load_registry_unreachable() {
    let result = SkillVerificationResult {
        verdict: SkillVerdict::RegistryUnreachable,
        score: None,
        findings: None,
    };
    assert!(should_load_skill(&result, true));
    assert!(should_load_skill(&result, false));
}