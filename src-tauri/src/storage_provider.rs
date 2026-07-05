//! Storage provider — the S3 data-plane's object-store + metering core. Objects
//! live in per-contract buckets; the provider computes the S3 `ETag` (MD5) and
//! the `x-amz-checksum-sha256` (base64 SHA-256), enforces the offer's max object
//! size, and prices egress and stored capacity.
//!
//! Design: `docs/chiral-book.md` → "Data-Plane API: Storage (S3)" and
//! "Provider Implementation" → Storage. The SigV4 / XML / HTTP wire layer wraps
//! this core; auth resolves a presented credential to a contract via the
//! session store, and the `egress_cost_wei` returned by `get` is charged with
//! `ContractLedger::draw_down`.
//!
//! v1 keeps objects in memory; a disk-backed store (`<data_dir>/provider/
//! storage/...`) is a drop-in behind the same API and is the production step.

use std::collections::HashMap;

use md5::Md5;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const GIB: u128 = 1024 * 1024 * 1024;
/// 30-day month for GB-month capacity pricing.
const SECONDS_PER_MONTH: u128 = 30 * 24 * 60 * 60;

#[derive(Debug, Clone)]
pub struct StoredObject {
    pub data: Vec<u8>,
    pub content_type: String,
    /// MD5 hex (the S3 ETag for a single-part put).
    pub etag: String,
    /// Base64 SHA-256 (the `x-amz-checksum-sha256` value).
    pub checksum_sha256: String,
    pub created: u64,
}

/// Metadata for HEAD / list responses (no body).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectMeta {
    pub key: String,
    pub size: u64,
    pub content_type: String,
    pub etag: String,
    pub checksum_sha256: String,
    pub created: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutResult {
    pub etag: String,
    pub checksum_sha256: String,
    pub size: u64,
}

/// Result of a GET, including the egress the caller must charge.
#[derive(Debug, Clone)]
pub struct GetResult {
    pub data: Vec<u8>,
    pub content_type: String,
    pub etag: String,
    pub checksum_sha256: String,
    pub egress_cost_wei: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageError {
    NoSuchKey,
    EntityTooLarge,
}

impl StorageError {
    /// The S3-native error `<Code>`.
    pub fn s3_code(&self) -> &'static str {
        match self {
            StorageError::NoSuchKey => "NoSuchKey",
            StorageError::EntityTooLarge => "EntityTooLarge",
        }
    }
    pub fn http_status(&self) -> u16 {
        match self {
            StorageError::NoSuchKey => 404,
            StorageError::EntityTooLarge => 400,
        }
    }
}

/// A provider's object store for one offer's pricing. Keyed by `(bucket, key)`.
#[derive(Debug)]
pub struct StorageProvider {
    objects: HashMap<(String, String), StoredObject>,
    pub per_gb_egress_wei: u128,
    pub per_gb_month_wei: u128,
    pub max_object_bytes: u64,
}

impl StorageProvider {
    pub fn new(per_gb_egress_wei: u128, per_gb_month_wei: u128, max_object_bytes: u64) -> Self {
        StorageProvider {
            objects: HashMap::new(),
            per_gb_egress_wei,
            per_gb_month_wei,
            max_object_bytes,
        }
    }

    /// Store an object, returning its ETag (MD5) and SHA-256 checksum. Ingress
    /// is not separately billed (capacity it creates is).
    pub fn put(
        &mut self,
        bucket: &str,
        key: &str,
        data: Vec<u8>,
        content_type: &str,
        now_unix: u64,
    ) -> Result<PutResult, StorageError> {
        if data.len() as u64 > self.max_object_bytes {
            return Err(StorageError::EntityTooLarge);
        }
        let etag = hex::encode(Md5::digest(&data));
        let checksum_sha256 = base64_std(&Sha256::digest(&data));
        let size = data.len() as u64;
        self.objects.insert(
            (bucket.to_string(), key.to_string()),
            StoredObject {
                data,
                content_type: content_type.to_string(),
                etag: etag.clone(),
                checksum_sha256: checksum_sha256.clone(),
                created: now_unix,
            },
        );
        Ok(PutResult {
            etag,
            checksum_sha256,
            size,
        })
    }

    /// Fetch an object and the egress cost the caller must charge (ceil-rounded
    /// so the provider never under-collects).
    pub fn get(&self, bucket: &str, key: &str) -> Result<GetResult, StorageError> {
        let o = self
            .objects
            .get(&(bucket.to_string(), key.to_string()))
            .ok_or(StorageError::NoSuchKey)?;
        Ok(GetResult {
            data: o.data.clone(),
            content_type: o.content_type.clone(),
            etag: o.etag.clone(),
            checksum_sha256: o.checksum_sha256.clone(),
            egress_cost_wei: self.egress_cost_wei(o.data.len() as u64),
        })
    }

    pub fn head(&self, bucket: &str, key: &str) -> Result<ObjectMeta, StorageError> {
        let o = self
            .objects
            .get(&(bucket.to_string(), key.to_string()))
            .ok_or(StorageError::NoSuchKey)?;
        Ok(ObjectMeta {
            key: key.to_string(),
            size: o.data.len() as u64,
            content_type: o.content_type.clone(),
            etag: o.etag.clone(),
            checksum_sha256: o.checksum_sha256.clone(),
            created: o.created,
        })
    }

    /// Delete an object. Idempotent (S3 delete succeeds whether or not the key
    /// existed).
    pub fn delete(&mut self, bucket: &str, key: &str) {
        self.objects.remove(&(bucket.to_string(), key.to_string()));
    }

    /// List a bucket's objects under an optional prefix, sorted by key.
    pub fn list(&self, bucket: &str, prefix: &str) -> Vec<ObjectMeta> {
        let mut out: Vec<ObjectMeta> = self
            .objects
            .iter()
            .filter(|((b, k), _)| b == bucket && k.starts_with(prefix))
            .map(|((_, k), o)| ObjectMeta {
                key: k.clone(),
                size: o.data.len() as u64,
                content_type: o.content_type.clone(),
                etag: o.etag.clone(),
                checksum_sha256: o.checksum_sha256.clone(),
                created: o.created,
            })
            .collect();
        out.sort_by(|a, b| a.key.cmp(&b.key));
        out
    }

    /// Egress price for `bytes`, ceil-rounded to wei.
    pub fn egress_cost_wei(&self, bytes: u64) -> u128 {
        ceil_div(bytes as u128 * self.per_gb_egress_wei, GIB)
    }

    /// Total bytes stored in a bucket (input to capacity metering).
    pub fn stored_bytes(&self, bucket: &str) -> u64 {
        self.objects
            .iter()
            .filter(|((b, _), _)| b == bucket)
            .map(|(_, o)| o.data.len() as u64)
            .sum()
    }

    /// Capacity cost (GB-month prorated) for `bucket` over `seconds`, ceil-wei.
    /// A periodic sampler calls this and charges the result via the ledger.
    pub fn capacity_cost_wei(&self, bucket: &str, seconds: u64) -> u128 {
        let bytes = self.stored_bytes(bucket) as u128;
        ceil_div(
            bytes * self.per_gb_month_wei * seconds as u128,
            GIB * SECONDS_PER_MONTH,
        )
    }
}

fn ceil_div(n: u128, d: u128) -> u128 {
    if d == 0 {
        return 0;
    }
    (n + d - 1) / d
}

fn base64_std(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PER_GB_EGRESS: u128 = 1_000_000_000_000_000; // 0.001 CHI / GB
    const PER_GB_MONTH: u128 = 10_000_000_000_000_000; // 0.01 CHI / GB-month

    fn provider() -> StorageProvider {
        StorageProvider::new(PER_GB_EGRESS, PER_GB_MONTH, 5_000_000)
    }

    #[test]
    fn put_get_roundtrip_with_known_hashes() {
        let mut p = provider();
        // Empty object: known MD5 + SHA-256 vectors.
        let r = p.put("c-x", "empty", vec![], "application/octet-stream", 100).unwrap();
        assert_eq!(r.etag, "d41d8cd98f00b204e9800998ecf8427e"); // md5("")
        assert_eq!(r.checksum_sha256, "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU="); // b64 sha256("")
        assert_eq!(r.size, 0);

        let g = p.get("c-x", "empty").unwrap();
        assert_eq!(g.data, Vec::<u8>::new());
        assert_eq!(g.etag, r.etag);
    }

    #[test]
    fn data_roundtrips_and_egress_priced() {
        let mut p = provider();
        let data = vec![7u8; 2048];
        p.put("c-x", "k", data.clone(), "text/plain", 1).unwrap();
        let g = p.get("c-x", "k").unwrap();
        assert_eq!(g.data, data);
        // 2048 bytes egress: ceil(2048 * rate / GiB) = 1 wei (tiny but non-zero).
        assert_eq!(g.egress_cost_wei, ceil_div(2048 * PER_GB_EGRESS, GIB));
        assert!(g.egress_cost_wei >= 1);
    }

    #[test]
    fn oversize_rejected() {
        let mut p = provider();
        let big = vec![0u8; 5_000_001];
        assert_eq!(
            p.put("c-x", "big", big, "application/octet-stream", 1).unwrap_err(),
            StorageError::EntityTooLarge
        );
    }

    #[test]
    fn missing_key_and_delete() {
        let mut p = provider();
        assert_eq!(p.get("c-x", "nope").unwrap_err(), StorageError::NoSuchKey);
        p.put("c-x", "k", vec![1, 2, 3], "application/octet-stream", 1).unwrap();
        assert!(p.head("c-x", "k").is_ok());
        p.delete("c-x", "k");
        assert_eq!(p.get("c-x", "k").unwrap_err(), StorageError::NoSuchKey);
        p.delete("c-x", "k"); // idempotent, no panic
    }

    #[test]
    fn list_by_prefix_sorted_and_bucket_scoped() {
        let mut p = provider();
        p.put("c-x", "a/1", vec![0; 10], "t", 1).unwrap();
        p.put("c-x", "a/2", vec![0; 20], "t", 1).unwrap();
        p.put("c-x", "b/1", vec![0; 30], "t", 1).unwrap();
        p.put("c-y", "a/9", vec![0; 40], "t", 1).unwrap(); // other bucket
        let listed = p.list("c-x", "a/");
        assert_eq!(listed.iter().map(|m| m.key.as_str()).collect::<Vec<_>>(), vec!["a/1", "a/2"]);
    }

    #[test]
    fn capacity_metering() {
        let mut p = provider();
        let size = 1024 * 1024; // 1 MiB, within max_object_bytes
        p.put("c-x", "k", vec![0u8; size], "t", 1).unwrap();
        assert_eq!(p.stored_bytes("c-x"), size as u64);
        // Full-month cost = bytes * per_gb_month / GiB (the seconds cancel), so a
        // full GiB-month is exactly PER_GB_MONTH by construction.
        let cost_month = p.capacity_cost_wei("c-x", SECONDS_PER_MONTH as u64);
        assert_eq!(cost_month, ceil_div(size as u128 * PER_GB_MONTH, GIB));
        assert!(cost_month > 0);
        // Half a month costs less.
        let cost_half = p.capacity_cost_wei("c-x", (SECONDS_PER_MONTH / 2) as u64);
        assert!(cost_half > 0 && cost_half < cost_month);
    }
}
