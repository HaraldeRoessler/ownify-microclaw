// Trust module — Ed25519 key loading, VC verification, DID resolution.
// Used by trust-aware tools (present_credential, verify_credential,
// log_compliance_action, check_delegation) and by the A2A auto-attach
// feature in tools/a2a.rs.

use ed25519_dalek::{Signature, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use base64::Engine;

#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub verified: bool,
    pub error: Option<String>,
}

impl VerifyResult {
    pub fn ok() -> Self {
        Self { verified: true, error: None }
    }
    pub fn fail(msg: impl Into<String>) -> Self {
        Self { verified: false, error: Some(msg.into()) }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TrustError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Base64 decode error: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("Multibase decode error: {0}")]
    Multibase(#[from] multibase::Error),
    #[error("Ed25519 error: {0}")]
    Ed25519(#[from] ed25519_dalek::SignatureError),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("Invalid key: {0}")]
    InvalidKey(String),
    #[error("DID not found: {0}")]
    DidNotFound(String),
}

/// Load an Ed25519 signing key from a file (raw 32-byte private key).
pub fn load_ed25519_private_key(path: &str) -> Result<SigningKey, TrustError> {
    let data = std::fs::read(path)?;
    if data.len() == 32 {
        let bytes: [u8; 32] = data[..32].try_into()
            .map_err(|_| TrustError::InvalidKey("expected 32 bytes".into()))?;
        Ok(SigningKey::from_bytes(&bytes))
    } else if data.len() == 64 {
        // Combined public+private (ownify format) — take the private half
        let bytes: [u8; 32] = data[32..64].try_into()
            .map_err(|_| TrustError::InvalidKey("expected 32 bytes in second half".into()))?;
        Ok(SigningKey::from_bytes(&bytes))
    } else {
        Err(TrustError::InvalidKey(format!(
            "expected 32 or 64 bytes, got {}", data.len()
        )))
    }
}

/// Load an Ed25519 verifying key from a multibase-encoded string (z6Mk...).
pub fn load_ed25519_public_key_from_multibase(multibase_str: &str) -> Result<VerifyingKey, TrustError> {
    let (_base, data) = multibase::decode(multibase_str)?;
    // Strip the 2-byte Ed25519 multicodec prefix (0xed 0x01)
    if data.len() < 34 {
        return Err(TrustError::InvalidKey(format!(
            "decoded data too short: {} bytes", data.len()
        )));
    }
    let pub_bytes: [u8; 32] = data[2..34].try_into()
        .map_err(|_| TrustError::InvalidKey("expected 32 bytes after prefix".into()))?;
    Ok(VerifyingKey::from_bytes(&pub_bytes)?)
}

/// Canonicalize a JSON value by sorting keys recursively.
/// This is an approximation of RFC 8785 (JCS) — sufficient for VC verification.
/// For full RFC 8785 compliance, use a dedicated JCS crate.
pub fn canonicalize_json(value: &serde_json::Value) -> String {
    canonicalize_value(value)
}

fn canonicalize_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let mut entries: Vec<(&String, &serde_json::Value)> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            let parts: Vec<String> = entries
                .iter()
                .map(|(k, v)| format!("\"{}\":{}", escape_json_string(k), canonicalize_value(v)))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        serde_json::Value::Array(arr) => {
            let parts: Vec<String> = arr.iter().map(canonicalize_value).collect();
            format!("[{}]", parts.join(","))
        }
        serde_json::Value::String(s) => format!("\"{}\"", escape_json_string(s)),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
    }
}

fn escape_json_string(s: &str) -> String {
    s.replace('\\', "\\\\")
     .replace('"', "\\\"")
     .replace('\n', "\\n")
     .replace('\r', "\\r")
     .replace('\t', "\\t")
}

/// Verify a W3C Verifiable Credential locally (no API call).
/// Extracts the public key from the DID document, canonicalizes the VC
/// (without the proof field), and verifies the Ed25519 signature.
pub fn verify_vc(
    vc: &serde_json::Value,
    did_document: &serde_json::Value,
) -> Result<VerifyResult, TrustError> {
    let proof = match vc.get("proof") {
        Some(p) => p,
        None => return Ok(VerifyResult::fail("missing proof field")),
    };

    if proof.get("type").and_then(|t| t.as_str()) != Some("Ed25519Signature2020") {
        return Ok(VerifyResult::fail("unsupported proof type"));
    }

    // Check expiry
    if let Some(exp) = vc.get("expirationDate").and_then(|e| e.as_str()) {
        if let Ok(expiry) = chrono::DateTime::parse_from_rfc3339(exp) {
            if chrono::Utc::now() > expiry {
                return Ok(VerifyResult::fail("credential expired"));
            }
        }
    }

    // Get the verification method ID from the proof
    let vm_id = match proof.get("verificationMethod").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return Ok(VerifyResult::fail("missing verificationMethod in proof")),
    };

    // Find the matching verification method in the DID document
    let vm = did_document
        .get("verificationMethod")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter().find(|vm| vm.get("id").and_then(|id| id.as_str()) == Some(vm_id))
        });

    let vm = match vm {
        Some(m) => m,
        None => return Ok(VerifyResult::fail(format!(
            "verification method {} not found in DID document", vm_id
        ))),
    };

    let pub_multibase = match vm.get("publicKeyMultibase").and_then(|p| p.as_str()) {
        Some(p) => p,
        None => return Ok(VerifyResult::fail("missing publicKeyMultibase in verification method")),
    };

    let verifying_key = load_ed25519_public_key_from_multibase(pub_multibase)?;

    // Canonicalize the VC without the proof field
    let mut vc_without_proof = vc.clone();
    if let serde_json::Value::Object(ref mut map) = vc_without_proof {
        map.remove("proof");
    }
    let canonical = canonicalize_json(&vc_without_proof);
    let data = canonical.as_bytes();

    // Decode the signature from base64
    let proof_value = match proof.get("proofValue").and_then(|p| p.as_str()) {
        Some(p) => p,
        None => return Ok(VerifyResult::fail("missing proofValue in proof")),
    };
    let sig_bytes = base64::engine::general_purpose::STANDARD.decode(proof_value)?;
    let signature = Signature::from_slice(&sig_bytes)?;

    // Verify
    match verifying_key.verify(data, &signature) {
        Ok(()) => Ok(VerifyResult::ok()),
        Err(_) => Ok(VerifyResult::fail("signature verification failed — credential may have been tampered")),
    }
}

/// Fetch a DID document from a did:web URL.
/// did:web:ownify.tech -> https://ownify.tech/.well-known/did.json
/// did:web:ownify.tech:agents:agent-1 -> https://ownify.tech/agents/agent-1/did.json
pub async fn fetch_did_document(did: &str) -> Result<serde_json::Value, TrustError> {
    if !did.starts_with("did:web:") {
        return Err(TrustError::DidNotFound(format!("only did:web supported, got: {}", did)));
    }
    let path_parts: Vec<&str> = did.strip_prefix("did:web:").unwrap().split(':').collect();
    let domain = path_parts[0];
    let url = if path_parts.len() == 1 {
        format!("https://{}/.well-known/did.json", domain)
    } else {
        let sub_path = path_parts[1..].join("/");
        format!("https://{}/{}/did.json", domain, sub_path)
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| TrustError::Http(e.to_string()))?;

    let resp = client.get(&url).send().await
        .map_err(|e| TrustError::Http(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(TrustError::DidNotFound(format!(
            "DID document fetch failed: HTTP {} from {}", resp.status(), url
        )));
    }

    let doc: serde_json::Value = resp.json().await
        .map_err(|e| TrustError::Http(e.to_string()))?;

    Ok(doc)
}

/// Compute SHA-256 hash of data, return as hex string.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    result.iter().map(|b| format!("{:02x}", b)).collect()
}