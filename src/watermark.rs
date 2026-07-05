// EU AI Act Article 50 — invisible watermark for AI-generated text.
// Encodes "OWNIFY|agent_slug|short_hash" as zero-width Unicode characters
// that are invisible to humans but machine-detectable.
//
// Encoding scheme:
//   U+200B (zero-width space) = bit 0
//   U+200C (zero-width non-joiner) = bit 1
//   U+200D (zero-width joiner) = start/end delimiter
//
// Watermark format: U+200D + binary("OWNIFY|slug|hash") + U+200D
// The hash is a short SHA-256 of the response text (first 8 hex chars).

use sha2::{Digest, Sha256};

const ZWSP: char = '\u{200B}'; // zero-width space = bit 0
const ZWNJ: char = '\u{200C}'; // zero-width non-joiner = bit 1
const ZWJ: char = '\u{200D}'; // zero-width joiner = delimiter

/// Add an invisible watermark to AI-generated text.
/// The watermark is appended at the end of the visible text.
pub fn add_watermark(text: &str, slug: &str) -> String {
    if text.is_empty() {
        return text.to_string();
    }

    // Compute short hash of the text
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let hash_hex = format!("{:x}", hasher.finalize());
    let short_hash = &hash_hex[..8.min(hash_hex.len())];

    // Build payload: OWNIFY|slug|hash
    let payload = format!("OWNIFY|{}|{}", slug, short_hash);

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
                    if decoded.starts_with("OWNIFY|") {
                        let parts: Vec<&str> = decoded.split('|').collect();
                        if parts.len() >= 3 {
                            return Some(WatermarkResult {
                                origin: parts[0].to_string(),
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
    // Remove all ZWJ...ZWJ sequences
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
    pub slug: String,
    pub hash: String,
    pub raw: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_detect_watermark() {
        let text = "Hello, I'm an AI agent.";
        let slug = "ownify-test-agent";
        let watermarked = add_watermark(text, slug);
        
        // Watermarked text should look the same to humans
        // (zero-width chars are invisible but trim() removes them, so compare visually)
        let visible_part = strip_watermark(&watermarked);
        assert_eq!(visible_part, text);
        
        // But should contain the watermark
        let result = detect_watermark(&watermarked);
        assert!(result.is_some());
        let wm = result.unwrap();
        assert_eq!(wm.origin, "OWNIFY");
        assert_eq!(wm.slug, "ownify-test-agent");
        assert!(!wm.hash.is_empty());
    }

    #[test]
    fn test_strip_watermark() {
        let text = "Hello world";
        let watermarked = add_watermark(text, "test");
        let stripped = strip_watermark(&watermarked);
        assert_eq!(stripped, text);
    }

    #[test]
    fn test_no_watermark_in_plain_text() {
        let text = "Just a normal text without any watermark.";
        assert!(detect_watermark(text).is_none());
    }
}