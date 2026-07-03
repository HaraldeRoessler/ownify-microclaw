use microclaw::trust::{canonicalize_json, load_ed25519_public_key_from_multibase};
use ed25519_dalek::{SigningKey, Signer};
use serde_json::json;

fn generate_signing_key() -> SigningKey {
    let mut secret = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut secret);
    SigningKey::from_bytes(&secret)
}

#[test]
fn test_canonicalize_json_sorts_keys() {
    let input = json!({"b": 1, "a": 2});
    let result = canonicalize_json(&input);
    assert!(result.starts_with("{\"a\""));
}

#[test]
fn test_canonicalize_json_nested() {
    let input = json!({"z": {"d": 1, "a": 2}});
    let result = canonicalize_json(&input);
    assert!(result.contains("\"a\":2"));
    assert!(result.contains("\"d\":1"));
}

#[test]
fn test_load_public_key_from_multibase() {
    let signing_key = generate_signing_key();
    let verifying_key = signing_key.verifying_key();
    let pub_bytes = verifying_key.to_bytes();

    let prefixed = [[0xed, 0x01].as_ref(), pub_bytes.as_ref()].concat();
    let encoded = multibase::encode(multibase::Base::Base58Btc, &prefixed);

    let decoded = load_ed25519_public_key_from_multibase(&encoded).unwrap();
    assert_eq!(decoded.to_bytes(), pub_bytes);
}

#[test]
fn test_sign_and_verify_roundtrip() {
    let signing_key = generate_signing_key();
    let verifying_key = signing_key.verifying_key();
    let data = b"test message for signing";
    let signature = signing_key.sign(data);

    use ed25519_dalek::Verifier;
    assert!(verifying_key.verify(data, &signature).is_ok());
}