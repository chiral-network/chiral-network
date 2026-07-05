//! Shared canonical-encoding helpers for the resource exchange: the primitives
//! used to build signed records (offers, receipts) and on-chain commitments
//! (contract-transaction `data`) deterministically on both sides of the wire.
//!
//! Design: `docs/chiral-book.md`, "Wire Protocol & API Reference" → Conventions.

use tiny_keccak::{Hasher, Keccak};

/// keccak256 digest.
pub fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut k = Keccak::v256();
    k.update(data);
    let mut out = [0u8; 32];
    k.finalize(&mut out);
    out
}

/// Append a little-endian `u32` length prefix followed by the bytes. This is
/// the codebase's established length-prefixing convention (see
/// `dht::file_info_sign_payload`); it makes a concatenation of fields injective
/// so no two distinct field tuples produce the same signed bytes.
pub fn put_lp(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
}

/// Deterministic JSON: object keys sorted recursively, compact output (no
/// whitespace). Ensures structured fields (`capacity`, `price_schedule`,
/// contract `params`/`rates`, …) hash identically regardless of the
/// serializer's key ordering — robust even if `serde_json`'s `preserve_order`
/// feature is ever enabled.
pub fn canonical_json(v: &serde_json::Value) -> String {
    fn canon(v: &serde_json::Value) -> serde_json::Value {
        match v {
            serde_json::Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let mut sorted = serde_json::Map::new();
                for k in keys {
                    sorted.insert(k.clone(), canon(&map[k]));
                }
                serde_json::Value::Object(sorted)
            }
            serde_json::Value::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(canon).collect())
            }
            other => other.clone(),
        }
    }
    serde_json::to_string(&canon(v)).unwrap_or_default()
}

/// Decode a `0x`-prefixed (or bare) hex string into a fixed `N`-byte array.
pub fn hex_to_array<const N: usize>(s: &str) -> Result<[u8; N], String> {
    let bytes = hex::decode(s.trim_start_matches("0x")).map_err(|e| format!("invalid hex: {}", e))?;
    if bytes.len() != N {
        return Err(format!("expected {} bytes, got {}", N, bytes.len()));
    }
    let mut out = [0u8; N];
    out.copy_from_slice(&bytes);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_json_sorts_keys_recursively() {
        let v = json!({ "b": 1, "a": 2, "nested": { "y": 1, "x": 2 }, "arr": [ {"q":1,"p":2} ] });
        assert_eq!(
            canonical_json(&v),
            r#"{"a":2,"arr":[{"p":2,"q":1}],"b":1,"nested":{"x":2,"y":1}}"#
        );
    }

    #[test]
    fn put_lp_is_length_prefixed() {
        let mut out = Vec::new();
        put_lp(&mut out, b"abc");
        assert_eq!(out, vec![3, 0, 0, 0, b'a', b'b', b'c']);
    }

    #[test]
    fn keccak256_known_vector() {
        // keccak256("") = c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470
        assert_eq!(
            hex::encode(keccak256(b"")),
            "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470"
        );
    }

    #[test]
    fn hex_to_array_roundtrip_and_errors() {
        let a: [u8; 4] = hex_to_array("0x01020304").unwrap();
        assert_eq!(a, [1, 2, 3, 4]);
        assert!(hex_to_array::<4>("0x0102").is_err()); // wrong length
        assert!(hex_to_array::<4>("0xzz020304").is_err()); // bad hex
    }
}
