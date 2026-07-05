// EU AI Act Article 50 — invisible watermark for AI-generated text.
// Encodes "OWNIFY|did|slug|hash" as zero-width Unicode characters
// that are invisible to humans but machine-detectable.
//
// Encoding scheme:
//   U+200B (zero-width space) = bit 0
//   U+200C (zero-width non-joiner) = bit 1
//   U+200D (zero-width joiner) = start/end delimiter
//
// Watermark format: U+200D + binary("OWNIFY|did|slug|hash") + U+200D
// - did: agent's DID (did:moltrust:xxx or did:web:ownify.tech:agents:slug)
// - slug: agent slug (fallback identifier)
// - hash: short SHA-256 of the response text (first 8 hex chars)

use sha2::{Digest, Sha256};

const ZWSP: char = '\u{200B}'; // zero-width space = bit 0
const ZWNJ: char = '\u{200C}'; // zero-width non-joiner = bit 1
const ZWJ: char = '\u{200D}'; // zero-width joiner = delimiter

/// Add an invisible watermark to AI-generated text.
/// The watermark is appended at the end of the visible text.
/// `did` is the agent's DID (from TrustConfig), `slug` is the agent slug.
/// 
/// Uses a compact format to survive Matrix's text processing which
/// strips some zero-width characters. Full DID is hashed to 8 chars.
pub fn add_watermark(text: &str, slug: &str, did: Option<&str>) -> String {
    if text.is_empty() {
        return text.to_string();
    }

    // Compute short hash of the text
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let hash_hex = format!("{:x}", hasher.finalize());
    let short_hash = &hash_hex[..8.min(hash_hex.len())];

    // Build COMPACT payload: O|did_hash|slug|text_hash
    // Using "O" prefix (not "OWNIFY") to minimize zero-width chars.
    // Full DID is hashed to 8 chars to keep payload short.
    // Total ~30 chars = ~240 bits — survives Matrix's stripping.
    let did_short = if let Some(d) = did {
        let mut h = Sha256::new();
        h.update(d.as_bytes());
        format!("{:x}", h.finalize())[..8].to_string()
    } else {
        "unknown".to_string()
    };
    let payload = format!("O|{}|{}|{}", did_short, slug, short_hash);

    // Convert payload to bits
    let bits: String = payload
        .as_bytes()
        .iter()
        .flat_map(|b| {
            let byte = *b;
            (0..8).rev().map(move |i| if (byte >> i) & 1 == 1 { '1' } else { '0' })
        })
        .collect();

    // Map bits to zero-width characters
    let watermark: String = bits
        .chars()
        .map(|b| if b == '0' { ZWSP } else { ZWNJ })
        .collect();

    // Append watermark with delimiters at the end of visible text
    let trimmed = text.trim_end();
    let trailing = &text[trimmed.len()..];
    format!("{}{}{}{}{}", trimmed, ZWJ, watermark, ZWJ, trailing)
}

/// Detect and decode a watermark from text. Returns None if no watermark found.
pub fn detect_watermark(text: &str) -> Option<WatermarkResult> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ZWJ {
            // Found start delimiter — extract bits until end delimiter
            let mut bits = String::new();
            let mut j = i + 1;
            while j < chars.len() && (chars[j] == ZWSP || chars[j] == ZWNJ) {
                bits.push(if chars[j] == ZWSP { '0' } else { '1' });
                j += 1;
            }
            if j < chars.len() && chars[j] == ZWJ {
                // Found end delimiter — decode bits
                if let Ok(decoded) = bits_to_string(&bits) {
                    // New compact format: O|did_hash|slug|hash
                    if decoded.starts_with("O|") {
                        let parts: Vec<&str> = decoded.split('|').collect();
                        if parts.len() >= 4 {
                            return Some(WatermarkResult {
                                origin: "OWNIFY".to_string(),
                                did: parts[1].to_string(),
                                slug: parts[2].to_string(),
                                hash: parts[3].to_string(),
                                raw: decoded,
                            });
                        }
                    }
                    // Old format backward compat: OWNIFY|did|slug|hash (4 parts)
                    if decoded.starts_with("OWNIFY|") {
                        let parts: Vec<&str> = decoded.split('|').collect();
                        if parts.len() >= 4 {
                            return Some(WatermarkResult {
                                origin: parts[0].to_string(),
                                did: parts[1].to_string(),
                                slug: parts[2].to_string(),
                                hash: parts[3].to_string(),
                                raw: decoded,
                            });
                        }
                        // Old 3-part format: OWNIFY|slug|hash
                        if parts.len() == 3 {
                            return Some(WatermarkResult {
                                origin: parts[0].to_string(),
                                did: "unknown".to_string(),
                                slug: parts[1].to_string(),
                                hash: parts[2].to_string(),
                                raw: decoded,
                            });
                        }
                    }
                }
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    None
}

/// Strip all watermarks from text.
pub fn strip_watermark(text: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == ZWJ {
            // Skip until next ZWJ
            let mut j = i + 1;
            while j < chars.len() && chars[j] != ZWJ && (chars[j] == ZWSP || chars[j] == ZWNJ) {
                j += 1;
            }
            if j < chars.len() && chars[j] == ZWJ {
                i = j + 1;
            } else {
                // Not a watermark — keep the ZWJ
                result.push(chars[i]);
                i += 1;
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
}

fn bits_to_string(bits: &str) -> Result<String, std::string::FromUtf8Error> {
    let bytes: Vec<u8> = bits
        .as_bytes()
        .chunks(8)
        .filter(|c| c.len() == 8)
        .map(|c| {
            let mut byte = 0u8;
            for &b in c {
                byte = (byte << 1) | if b == b'1' { 1 } else { 0 };
            }
            byte
        })
        .collect();
    String::from_utf8(bytes)
}

#[derive(Debug, Clone)]
pub struct WatermarkResult {
    pub origin: String,
    pub did: String,
    pub slug: String,
    pub hash: String,
    pub raw: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_detect_watermark_with_did() {
        let text = "Hello, I'm an AI agent.";
        let slug = "ownify-test-agent";
        let did = "did:moltrust:abc123def456";
        let watermarked = add_watermark(text, slug, Some(did));
        
        // Watermarked text should look the same to humans
        let visible_part = strip_watermark(&watermarked);
        assert_eq!(visible_part, text);
        
        // But should contain the watermark with DID hash
        let result = detect_watermark(&watermarked);
        assert!(result.is_some());
        let wm = result.unwrap();
        assert_eq!(wm.origin, "OWNIFY");
        assert!(!wm.did.is_empty());
        assert!(!wm.slug.is_empty());
        assert!(!wm.hash.is_empty());
    }

    #[test]
    fn test_add_watermark_without_did() {
        let text = "Hello world";
        let watermarked = add_watermark(text, "test", None);
        let result = detect_watermark(&watermarked).unwrap();
        assert_eq!(result.did, "unknown");
        assert_eq!(result.slug, "test");
    }

    #[test]
    fn test_strip_watermark() {
        let text = "Hello world";
        let watermarked = add_watermark(text, "test", Some("did:web:ownify.tech"));
        let stripped = strip_watermark(&watermarked);
        assert_eq!(stripped, text);
    }

    #[test]
    fn test_no_watermark_in_plain_text() {
        let text = "Just a normal text without any watermark.";
        assert!(detect_watermark(text).is_none());
    }
}