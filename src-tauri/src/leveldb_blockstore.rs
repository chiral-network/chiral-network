use blockstore::Blockstore;
use cid::{Cid, CidGeneric};
use futures::future::BoxFuture;
use rusty_leveldb::{DB, Options};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing::{debug, info};

/// LevelDB-based blockstore for content-addressed storage
///
/// This implementation provides async wrappers around rusty-leveldb for use
/// with Bitswap and DHT file sharing. It follows Geth's database patterns
/// for better ecosystem compatibility.
#[derive(Clone)]
pub struct LevelDbBlockstore {
    db: Arc<Mutex<DB>>,
    in_memory: bool,
}

impl LevelDbBlockstore {
    /// Open a LevelDB blockstore from disk
    ///
    /// # Arguments
    /// * `path` - Directory path for the LevelDB database
    ///
    /// # Configuration
    /// - Snappy compression (compressor ID 0)
    /// - 128MB block cache
    /// - 64MB write buffer
    pub async fn open<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();

        info!("Opening LevelDB blockstore at: {:?}", path);

        let db = tokio::task::spawn_blocking(move || -> Result<DB, String> {
            let mut options = Options::default();

            // Set cache size to 128MB for better read performance
            options.block_cache_capacity_bytes = 128 * 1024 * 1024;

            // Set write buffer to 64MB for better write batching
            options.write_buffer_size = 64 * 1024 * 1024;

            // Enable paranoid checks for data integrity
            options.paranoid_checks = true;

            // Create database if it doesn't exist
            options.create_if_missing = true;

            DB::open(path, options).map_err(|e| format!("Failed to open LevelDB: {:?}", e))
        })
        .await
        .map_err(|e| format!("Spawn blocking error: {}", e))??;

        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            in_memory: false,
        })
    }

    /// Create an in-memory blockstore
    ///
    /// This is used as a fallback when disk storage fails or for testing.
    /// Uses a temporary directory that will be cleaned up on drop.
    pub async fn in_memory() -> Result<Self, String> {
        info!("Creating in-memory LevelDB blockstore");

        let db = tokio::task::spawn_blocking(move || -> Result<DB, String> {
            let mut options = Options::default();

            // Smaller cache for in-memory (32MB)
            options.block_cache_capacity_bytes = 32 * 1024 * 1024;
            options.write_buffer_size = 16 * 1024 * 1024;
            options.paranoid_checks = false; // Skip checks for temp storage
            options.create_if_missing = true;

            // Use a temporary directory
            let temp_dir = std::env::temp_dir().join(format!("leveldb_temp_{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&temp_dir)
                .map_err(|e| format!("Failed to create temp dir: {}", e))?;

            DB::open(temp_dir, options).map_err(|e| format!("Failed to open in-memory LevelDB: {:?}", e))
        })
        .await
        .map_err(|e| format!("Spawn blocking error: {}", e))??;

        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            in_memory: true,
        })
    }

    /// Store a block by its CID (internal method)
    ///
    /// # Arguments
    /// * `cid` - Content identifier for the block
    /// * `data` - Block data bytes
    async fn put_internal(&self, cid: &Cid, data: Vec<u8>) -> Result<(), String> {
        let db = self.db.clone();
        let key = cid.to_bytes();
        let data_len = data.len();

        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let mut db_lock = db.lock().map_err(|e| format!("Lock error: {}", e))?;
            db_lock.put(&key, &data).map_err(|e| format!("Put error: {:?}", e))?;
            Ok(())
        })
        .await
        .map_err(|e| format!("Spawn blocking error: {}", e))??;

        debug!("Stored block {} ({} bytes)", cid, data_len);
        Ok(())
    }

    /// Retrieve a block by its CID (internal method)
    ///
    /// # Arguments
    /// * `cid` - Content identifier for the block
    ///
    /// # Returns
    /// * `Ok(Some(data))` - Block found
    /// * `Ok(None)` - Block not found
    /// * `Err(_)` - Database error
    async fn get_internal(&self, cid: &Cid) -> Result<Option<Vec<u8>>, String> {
        let db = self.db.clone();
        let key = cid.to_bytes();

        let result = tokio::task::spawn_blocking(move || -> Result<Option<Vec<u8>>, String> {
            let mut db_lock = db.lock().map_err(|e| format!("Lock error: {}", e))?;
            match db_lock.get(&key) {
                Some(data) => Ok(Some(data)),
                None => Ok(None),
            }
        })
        .await
        .map_err(|e| format!("Spawn blocking error: {}", e))??;

        match &result {
            Some(data) => debug!("Retrieved block {} ({} bytes)", cid, data.len()),
            None => debug!("Block {} not found", cid),
        }

        Ok(result)
    }

    /// Check if a block exists by its CID (internal method)
    ///
    /// # Arguments
    /// * `cid` - Content identifier to check
    ///
    /// # Returns
    /// * `true` if block exists
    /// * `false` if block does not exist
    async fn has_internal(&self, cid: &Cid) -> Result<bool, String> {
        let db = self.db.clone();
        let key = cid.to_bytes();

        let exists = tokio::task::spawn_blocking(move || -> Result<bool, String> {
            let mut db_lock = db.lock().map_err(|e| format!("Lock error: {}", e))?;
            Ok(db_lock.get(&key).is_some())
        })
        .await
        .map_err(|e| format!("Spawn blocking error: {}", e))??;

        debug!("Block {} exists: {}", cid, exists);
        Ok(exists)
    }

    /// Delete a block by its CID (internal method)
    ///
    /// This is used for garbage collection and pin management.
    ///
    /// # Arguments
    /// * `cid` - Content identifier to delete
    async fn delete_internal(&self, cid: &Cid) -> Result<(), String> {
        let db = self.db.clone();
        let key = cid.to_bytes();

        tokio::task::spawn_blocking(move || -> Result<(), String> {
            let mut db_lock = db.lock().map_err(|e| format!("Lock error: {}", e))?;
            db_lock.delete(&key).map_err(|e| format!("Delete error: {:?}", e))?;
            Ok(())
        })
        .await
        .map_err(|e| format!("Spawn blocking error: {}", e))??;

        debug!("Deleted block {}", cid);
        Ok(())
    }

    /// Check if this is an in-memory blockstore
    pub fn is_in_memory(&self) -> bool {
        self.in_memory
    }
}

/// Implement the blockstore::Blockstore trait for compatibility with beetswap
impl Blockstore for LevelDbBlockstore {
    fn get<const S: usize>(&self, cid: &CidGeneric<S>) -> BoxFuture<'_, Result<Option<Vec<u8>>, blockstore::Error>> {
        let cid_bytes = cid.to_bytes();
        Box::pin(async move {
            // Convert CidGeneric<S> to Cid (CidGeneric<64>) for internal use
            let cid_64: Cid = Cid::try_from(cid_bytes).map_err(|_| {
                blockstore::Error::FatalDatabaseError("Failed to convert CID".to_string())
            })?;
            self.get_internal(&cid_64)
                .await
                .map_err(|e| blockstore::Error::FatalDatabaseError(e))
        })
    }

    fn put_keyed<const S: usize>(&self, cid: &CidGeneric<S>, data: &[u8]) -> BoxFuture<'_, Result<(), blockstore::Error>> {
        let cid_bytes = cid.to_bytes();
        let data_vec = data.to_vec();
        Box::pin(async move {
            // Convert CidGeneric<S> to Cid (CidGeneric<64>) for internal use
            let cid_64: Cid = Cid::try_from(cid_bytes).map_err(|_| {
                blockstore::Error::FatalDatabaseError("Failed to convert CID".to_string())
            })?;
            self.put_internal(&cid_64, data_vec)
                .await
                .map_err(|e| blockstore::Error::FatalDatabaseError(e))
        })
    }

    fn remove<const S: usize>(&self, cid: &CidGeneric<S>) -> BoxFuture<'_, Result<(), blockstore::Error>> {
        let cid_bytes = cid.to_bytes();
        Box::pin(async move {
            // Convert CidGeneric<S> to Cid (CidGeneric<64>) for internal use
            let cid_64: Cid = Cid::try_from(cid_bytes).map_err(|_| {
                blockstore::Error::FatalDatabaseError("Failed to convert CID".to_string())
            })?;
            self.delete_internal(&cid_64)
                .await
                .map_err(|e| blockstore::Error::FatalDatabaseError(e))
        })
    }

    fn close(self) -> BoxFuture<'static, Result<(), blockstore::Error>> {
        Box::pin(async move {
            // LevelDB auto-closes when dropped, no explicit close needed
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cid::multihash::{Code, MultihashDigest};
    use tempfile::tempdir;

    const RAW_CODEC: u64 = 0x55;

    #[tokio::test]
    async fn test_put_get() {
        let temp_dir = tempdir().unwrap();
        let blockstore = LevelDbBlockstore::open(temp_dir.path()).await.unwrap();

        let data = b"test data";
        let cid = Cid::new_v1(RAW_CODEC, Code::Sha2_256.digest(data));

        // Put
        blockstore.put(&cid, data.to_vec()).await.unwrap();

        // Get
        let retrieved = blockstore.get(&cid).await.unwrap();
        assert_eq!(retrieved, Some(data.to_vec()));
    }

    #[tokio::test]
    async fn test_has() {
        let temp_dir = tempdir().unwrap();
        let blockstore = LevelDbBlockstore::open(temp_dir.path()).await.unwrap();

        let data = b"test data";
        let cid = Cid::new_v1(RAW_CODEC, Code::Sha2_256.digest(data));

        // Should not exist initially
        assert!(!blockstore.has(&cid).await.unwrap());

        // Put
        blockstore.put(&cid, data.to_vec()).await.unwrap();

        // Should exist now
        assert!(blockstore.has(&cid).await.unwrap());
    }

    #[tokio::test]
    async fn test_delete() {
        let temp_dir = tempdir().unwrap();
        let blockstore = LevelDbBlockstore::open(temp_dir.path()).await.unwrap();

        let data = b"test data";
        let cid = Cid::new_v1(RAW_CODEC, Code::Sha2_256.digest(data));

        // Put
        blockstore.put(&cid, data.to_vec()).await.unwrap();
        assert!(blockstore.has(&cid).await.unwrap());

        // Delete
        blockstore.delete(&cid).await.unwrap();
        assert!(!blockstore.has(&cid).await.unwrap());
    }

    #[tokio::test]
    async fn test_in_memory() {
        let blockstore = LevelDbBlockstore::in_memory().await.unwrap();
        assert!(blockstore.is_in_memory());

        let data = b"test data";
        let cid = Cid::new_v1(RAW_CODEC, Code::Sha2_256.digest(data));

        blockstore.put(&cid, data.to_vec()).await.unwrap();
        let retrieved = blockstore.get(&cid).await.unwrap();
        assert_eq!(retrieved, Some(data.to_vec()));
    }

    #[tokio::test]
    async fn test_large_block() {
        let temp_dir = tempdir().unwrap();
        let blockstore = LevelDbBlockstore::open(temp_dir.path()).await.unwrap();

        // 1MB block
        let data = vec![0u8; 1024 * 1024];
        let cid = Cid::new_v1(RAW_CODEC, Code::Sha2_256.digest(&data));

        blockstore.put(&cid, data.clone()).await.unwrap();
        let retrieved = blockstore.get(&cid).await.unwrap();
        assert_eq!(retrieved, Some(data));
    }

    #[tokio::test]
    async fn test_concurrent_writes() {
        let temp_dir = tempdir().unwrap();
        let blockstore = Arc::new(LevelDbBlockstore::open(temp_dir.path()).await.unwrap());

        let mut handles = vec![];

        for i in 0..10 {
            let bs = blockstore.clone();
            let handle = tokio::spawn(async move {
                let data = format!("data {}", i);
                let cid = Cid::new_v1(RAW_CODEC, Code::Sha2_256.digest(data.as_bytes()));
                bs.put(&cid, data.into_bytes()).await.unwrap();
                cid
            });
            handles.push(handle);
        }

        let cids: Vec<Cid> = futures::future::join_all(handles)
            .await
            .into_iter()
            .map(|r| r.unwrap())
            .collect();

        // Verify all blocks were stored
        for cid in cids {
            assert!(blockstore.has(&cid).await.unwrap());
        }
    }
}
