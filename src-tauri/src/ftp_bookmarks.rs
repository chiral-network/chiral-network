// ftp_bookmarks.rs
// FTP server bookmarks/favorites management
//
// This module provides functionality to save, load, and manage FTP server
// bookmarks for quick access to frequently used servers.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tracing::{debug, info};

/// FTP server bookmark
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtpBookmark {
    /// Unique identifier for the bookmark
    pub id: String,

    /// User-friendly name for the bookmark
    pub name: String,

    /// FTP server URL
    pub url: String,

    /// FTP username
    pub username: Option<String>,

    /// Encrypted password (optional)
    pub encrypted_password: Option<String>,

    /// Whether to use FTPS
    #[serde(default)]
    pub use_ftps: bool,

    /// Whether to use passive mode
    #[serde(default = "default_passive_mode")]
    pub passive_mode: bool,

    /// Custom port (if different from default)
    pub port: Option<u16>,

    /// Notes/description for this bookmark
    pub notes: Option<String>,

    /// Tags for categorization
    #[serde(default)]
    pub tags: Vec<String>,

    /// Last used timestamp (Unix timestamp)
    pub last_used: Option<i64>,

    /// Number of times this bookmark was used
    #[serde(default)]
    pub use_count: u32,
}

fn default_passive_mode() -> bool {
    true
}

/// FTP bookmarks manager
pub struct FtpBookmarksManager {
    bookmarks_file: PathBuf,
}

impl FtpBookmarksManager {
    /// Create a new bookmarks manager
    pub fn new(config_dir: PathBuf) -> Self {
        let bookmarks_file = config_dir.join("ftp_bookmarks.json");
        Self { bookmarks_file }
    }

    /// Load all bookmarks from disk
    pub fn load_bookmarks(&self) -> Result<Vec<FtpBookmark>> {
        if !self.bookmarks_file.exists() {
            debug!("Bookmarks file does not exist, returning empty list");
            return Ok(Vec::new());
        }

        let contents = fs::read_to_string(&self.bookmarks_file)
            .context("Failed to read bookmarks file")?;

        let bookmarks: Vec<FtpBookmark> = serde_json::from_str(&contents)
            .context("Failed to parse bookmarks JSON")?;

        info!(count = bookmarks.len(), "Loaded FTP bookmarks");

        Ok(bookmarks)
    }

    /// Save all bookmarks to disk
    pub fn save_bookmarks(&self, bookmarks: &[FtpBookmark]) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = self.bookmarks_file.parent() {
            fs::create_dir_all(parent).context("Failed to create config directory")?;
        }

        let json = serde_json::to_string_pretty(bookmarks)
            .context("Failed to serialize bookmarks")?;

        fs::write(&self.bookmarks_file, json)
            .context("Failed to write bookmarks file")?;

        info!(count = bookmarks.len(), "Saved FTP bookmarks");

        Ok(())
    }

    /// Add a new bookmark
    pub fn add_bookmark(&self, bookmark: FtpBookmark) -> Result<Vec<FtpBookmark>> {
        let mut bookmarks = self.load_bookmarks()?;

        // Check for duplicate ID
        if bookmarks.iter().any(|b| b.id == bookmark.id) {
            anyhow::bail!("Bookmark with ID '{}' already exists", bookmark.id);
        }

        bookmarks.push(bookmark);
        self.save_bookmarks(&bookmarks)?;

        Ok(bookmarks)
    }

    /// Update an existing bookmark
    pub fn update_bookmark(&self, bookmark: FtpBookmark) -> Result<Vec<FtpBookmark>> {
        let mut bookmarks = self.load_bookmarks()?;

        let index = bookmarks
            .iter()
            .position(|b| b.id == bookmark.id)
            .context("Bookmark not found")?;

        bookmarks[index] = bookmark;
        self.save_bookmarks(&bookmarks)?;

        Ok(bookmarks)
    }

    /// Delete a bookmark by ID
    pub fn delete_bookmark(&self, id: &str) -> Result<Vec<FtpBookmark>> {
        let mut bookmarks = self.load_bookmarks()?;

        let index = bookmarks
            .iter()
            .position(|b| b.id == id)
            .context("Bookmark not found")?;

        bookmarks.remove(index);
        self.save_bookmarks(&bookmarks)?;

        info!(id = %id, "Deleted FTP bookmark");

        Ok(bookmarks)
    }

    /// Get a bookmark by ID
    pub fn get_bookmark(&self, id: &str) -> Result<Option<FtpBookmark>> {
        let bookmarks = self.load_bookmarks()?;
        Ok(bookmarks.into_iter().find(|b| b.id == id))
    }

    /// Update bookmark usage statistics
    pub fn record_usage(&self, id: &str) -> Result<()> {
        let mut bookmarks = self.load_bookmarks()?;

        if let Some(bookmark) = bookmarks.iter_mut().find(|b| b.id == id) {
            bookmark.use_count += 1;
            bookmark.last_used = Some(chrono::Utc::now().timestamp());
            self.save_bookmarks(&bookmarks)?;
        }

        Ok(())
    }

    /// Search bookmarks by name, URL, or tags
    pub fn search_bookmarks(&self, query: &str) -> Result<Vec<FtpBookmark>> {
        let bookmarks = self.load_bookmarks()?;
        let query_lower = query.to_lowercase();

        let filtered: Vec<FtpBookmark> = bookmarks
            .into_iter()
            .filter(|b| {
                b.name.to_lowercase().contains(&query_lower)
                    || b.url.to_lowercase().contains(&query_lower)
                    || b.tags.iter().any(|t| t.to_lowercase().contains(&query_lower))
                    || b.notes.as_ref().map_or(false, |n| n.to_lowercase().contains(&query_lower))
            })
            .collect();

        Ok(filtered)
    }

    /// Get bookmarks sorted by usage (most used first)
    pub fn get_most_used(&self, limit: usize) -> Result<Vec<FtpBookmark>> {
        let mut bookmarks = self.load_bookmarks()?;
        bookmarks.sort_by(|a, b| b.use_count.cmp(&a.use_count));
        bookmarks.truncate(limit);
        Ok(bookmarks)
    }

    /// Get recently used bookmarks
    pub fn get_recently_used(&self, limit: usize) -> Result<Vec<FtpBookmark>> {
        let mut bookmarks = self.load_bookmarks()?;
        bookmarks.sort_by(|a, b| {
            b.last_used.unwrap_or(0).cmp(&a.last_used.unwrap_or(0))
        });
        bookmarks.truncate(limit);
        Ok(bookmarks)
    }

    /// Export bookmarks to JSON string
    pub fn export_bookmarks(&self) -> Result<String> {
        let bookmarks = self.load_bookmarks()?;
        serde_json::to_string_pretty(&bookmarks)
            .context("Failed to export bookmarks")
    }

    /// Import bookmarks from JSON string
    pub fn import_bookmarks(&self, json: &str, merge: bool) -> Result<Vec<FtpBookmark>> {
        let imported: Vec<FtpBookmark> = serde_json::from_str(json)
            .context("Failed to parse imported bookmarks")?;

        let bookmarks = if merge {
            let mut existing = self.load_bookmarks()?;

            // Merge: add new bookmarks, skip duplicates
            for bookmark in imported {
                if !existing.iter().any(|b| b.id == bookmark.id) {
                    existing.push(bookmark);
                }
            }

            existing
        } else {
            // Replace: use imported bookmarks
            imported
        };

        self.save_bookmarks(&bookmarks)?;
        Ok(bookmarks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_add_and_load_bookmark() {
        let temp_dir = TempDir::new().unwrap();
        let manager = FtpBookmarksManager::new(temp_dir.path().to_path_buf());

        let bookmark = FtpBookmark {
            id: "test1".to_string(),
            name: "Test Server".to_string(),
            url: "ftp://test.example.com".to_string(),
            username: Some("user".to_string()),
            encrypted_password: None,
            use_ftps: false,
            passive_mode: true,
            port: None,
            notes: Some("Test bookmark".to_string()),
            tags: vec!["test".to_string()],
            last_used: None,
            use_count: 0,
        };

        manager.add_bookmark(bookmark.clone()).unwrap();
        let loaded = manager.load_bookmarks().unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "test1");
        assert_eq!(loaded[0].name, "Test Server");
    }

    #[test]
    fn test_search_bookmarks() {
        let temp_dir = TempDir::new().unwrap();
        let manager = FtpBookmarksManager::new(temp_dir.path().to_path_buf());

        let bookmark1 = FtpBookmark {
            id: "test1".to_string(),
            name: "Production Server".to_string(),
            url: "ftp://prod.example.com".to_string(),
            username: None,
            encrypted_password: None,
            use_ftps: false,
            passive_mode: true,
            port: None,
            notes: None,
            tags: vec!["production".to_string()],
            last_used: None,
            use_count: 0,
        };

        let bookmark2 = FtpBookmark {
            id: "test2".to_string(),
            name: "Development Server".to_string(),
            url: "ftp://dev.example.com".to_string(),
            username: None,
            encrypted_password: None,
            use_ftps: false,
            passive_mode: true,
            port: None,
            notes: None,
            tags: vec!["development".to_string()],
            last_used: None,
            use_count: 0,
        };

        manager.add_bookmark(bookmark1).unwrap();
        manager.add_bookmark(bookmark2).unwrap();

        let results = manager.search_bookmarks("prod").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Production Server");
    }
}
