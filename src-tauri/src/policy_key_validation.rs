pub fn is_placeholder_policy_key(key: &[u8; 32]) -> bool {
    *key == [0u8; 32]
}

pub fn parse_policy_public_key_hex(raw: &str) -> Result<[u8; 32], String> {
    let trimmed = raw.trim();
    let cleaned = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);
    if cleaned.len() != 64 {
        return Err("expected exactly 64 hex characters".to_string());
    }

    let bytes =
        hex::decode(cleaned).map_err(|e| format!("key contains non-hex characters: {e}"))?;
    let key: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| "expected exactly 32 key bytes".to_string())?;
    if is_placeholder_policy_key(&key) {
        return Ok(key);
    }

    ed25519_dalek::VerifyingKey::from_bytes(&key)
        .map_err(|e| format!("invalid Ed25519 public key: {e}"))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    #[test]
    fn parses_valid_ed25519_public_key() {
        let signing = SigningKey::from_bytes(&[7u8; 32]);
        let public_hex = hex::encode(signing.verifying_key().to_bytes());

        let parsed = parse_policy_public_key_hex(&public_hex).unwrap();

        assert_eq!(parsed, signing.verifying_key().to_bytes());
    }

    #[test]
    fn rejects_malformed_nonzero_32_byte_key() {
        let invalid = hex::encode([0x42u8; 32]);

        let err = parse_policy_public_key_hex(&invalid).unwrap_err();

        assert!(err.contains("invalid Ed25519 public key"));
    }

    #[test]
    fn parses_placeholder_for_debug_build_callers() {
        let parsed = parse_policy_public_key_hex(&hex::encode([0u8; 32])).unwrap();

        assert!(is_placeholder_policy_key(&parsed));
    }
}
