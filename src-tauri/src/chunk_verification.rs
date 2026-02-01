//! Chunk integrity verification utilities.
//!
//! This module provides standalone hash verification for file chunks,
//! used by WebRTC downloads and multi-source download coordination.

use sha2::{Digest, Sha256};

/// Error returned when chunk hash verification fails.
#[derive(Debug, Clone)]
pub struct ChunkVerificationError {
    pub expected: String,
    pub actual: String,
}

impl std::fmt::Display for ChunkVerificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Chunk hash mismatch: expected {}, got {}",
            self.expected, self.actual
        )
    }
}

impl std::error::Error for ChunkVerificationError {}

/// Normalize and validate SHA-256 hash format (64 hex characters, lowercase).
///
/// Returns `None` if the hash is not a valid 64-character hex string.
pub fn normalize_sha256_hex(hash: &str) -> Option<String> {
    let trimmed = hash.trim();
    if trimmed.len() != 64 {
        return None;
    }

    if trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(trimmed.to_ascii_lowercase())
    } else {
        None
    }
}

/// Verify chunk data against an expected SHA-256 hash.
///
/// # Arguments
/// * `expected_hash` - The expected SHA-256 hash (64 hex characters)
/// * `data` - The chunk data to verify
///
/// # Returns
/// * `Ok(())` if the hash matches or if the expected hash is invalid format (skips verification)
/// * `Err(ChunkVerificationError)` if the hash does not match
pub fn verify_chunk_hash(expected_hash: &str, data: &[u8]) -> Result<(), ChunkVerificationError> {
    let expected = match normalize_sha256_hex(expected_hash) {
        Some(value) => value,
        None => return Ok(()), // Invalid hash format, skip verification
    };

    let mut hasher = Sha256::new();
    hasher.update(data);
    let actual = hex::encode(hasher.finalize());

    if actual != expected {
        return Err(ChunkVerificationError {
            expected: expected_hash.to_string(),
            actual,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_chunk_hash_valid() {
        let data = b"hello world";
        // SHA-256 of "hello world"
        let expected = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        assert!(verify_chunk_hash(expected, data).is_ok());
    }

    #[test]
    fn test_verify_chunk_hash_mismatch() {
        let data = b"hello world";
        let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";
        let result = verify_chunk_hash(wrong_hash, data);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.expected, wrong_hash);
    }

    #[test]
    fn test_verify_chunk_hash_invalid_format_skips() {
        let data = b"hello world";
        // Invalid hash format (too short) - should skip verification
        assert!(verify_chunk_hash("invalid", data).is_ok());
        assert!(verify_chunk_hash("", data).is_ok());
    }

    #[test]
    fn test_normalize_sha256_hex() {
        // Valid hash
        let hash = "B94D27B9934D3E08A52E52D7DA7DABFAC484EFE37A5380EE9088F7ACE2EFCDE9";
        assert_eq!(
            normalize_sha256_hex(hash),
            Some("b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9".to_string())
        );

        // Invalid (too short)
        assert_eq!(normalize_sha256_hex("abc"), None);

        // Invalid (non-hex characters)
        let invalid = "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz";
        assert_eq!(normalize_sha256_hex(invalid), None);
    }
}
