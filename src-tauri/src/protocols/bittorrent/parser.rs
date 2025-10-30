//! # .torrent and Magnet URI Parser
//!
//! This module is responsible for parsing BitTorrent-related data formats,
//! specifically `.torrent` files and magnet URIs.

use bendy::decoding::{self, Error as BendyError};
use serde::Deserialize;
use sha1::{Digest, Sha1};
use std::fs;
use std::path::Path;
use url::Url;

/// Represents a torrent, which can be from a `.torrent` file or a magnet URI.
#[derive(Debug)]
pub enum Torrent {
    /// A torrent parsed from a `.torrent` file, containing full metadata.
    File(TorrentMetadata),
    /// A torrent parsed from a magnet URI, containing essential link information.
    Magnet(Magnet),
}

/// Represents the data parsed from a magnet URI.
#[derive(Debug, PartialEq)]
pub struct Magnet {
    /// The display name of the torrent (`dn` parameter).
    pub display_name: Option<String>,
    /// The info hash of the torrent (`xt` parameter).
    pub info_hash: Vec<u8>,
    /// A list of tracker URLs (`tr` parameter).
    pub trackers: Vec<String>,
}

/// Represents the metadata contained within a .torrent file.
#[derive(Debug, Deserialize)]
pub struct TorrentMetadata {
    /// The URL of the tracker.
    pub announce: String,
    /// Information about the files in the torrent.
    pub info: TorrentInfo,
    /// The SHA1 hash of the bencoded `info` dictionary. This is the info hash.
    #[serde(skip)]
    pub info_hash: Vec<u8>,
}

/// Contains information about the file(s) in the torrent.
#[derive(Debug, Deserialize)]
pub struct TorrentInfo {
    /// The suggested name for the file or directory.
    pub name: String,
    /// The length of a single piece in bytes.
    #[serde(rename = "piece length")]
    pub piece_length: u64,
    /// A string of concatenated SHA1 hashes of each piece.
    pub pieces: Vec<u8>,
    /// Information about the file(s).
    #[serde(flatten)]
    pub files: FileInfo,
}

/// Represents either a single file or multiple files in a torrent.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum FileInfo {
    /// A single file torrent.
    Single {
        /// Length of the file in bytes.
        length: u64,
    },
    /// A multi-file torrent.
    Multi {
        /// A list of files.
        files: Vec<File>,
    },
}

/// Represents a single file in a multi-file torrent.
#[derive(Debug, Deserialize)]
pub struct File {
    /// The length of the file in bytes.
    pub length: u64,
    /// The path of the file.
    pub path: Vec<String>,
}

/// Parses a .torrent file from the given path.
///
/// # Arguments
///
/// * `file_path` - The path to the .torrent file.
///
/// # Returns
///
/// A `Result` containing the `TorrentMetadata` or an error string.
pub fn parse_torrent_file(file_path: &str) -> Result<TorrentMetadata, String> {
    let torrent_bytes = fs::read(Path::new(file_path))
        .map_err(|e| format!("Failed to read .torrent file: {}", e))?;

    // The `bendy` crate can't directly give us the raw `info` dictionary bytes
    // while deserializing the whole structure. We'll do a two-pass parse.
    // First, get the raw `info` value.
    #[derive(Deserialize)]
    struct RawInfoValue<'a> {
        #[serde(rename = "info", with = "bendy::serde::raw_value")]
        info: &'a [u8],
    }

    let raw_info_value: RawInfoValue = decoding::from_bytes(&torrent_bytes)
        .map_err(|e: BendyError| format!("Failed to parse .torrent for info hash: {}", e))?;

    // Calculate the info hash.
    let mut hasher = Sha1::new();
    hasher.update(raw_info_value.info);
    let info_hash = hasher.finalize().to_vec();

    // Now, parse the full metadata.
    let mut metadata: TorrentMetadata = decoding::from_bytes(&torrent_bytes)
        .map_err(|e: BendyError| format!("Failed to parse .torrent file: {}", e))?;

    metadata.info_hash = info_hash;

    Ok(metadata)
}

/// Parses a magnet URI.
///
/// # Arguments
///
/// * `uri` - The magnet URI string.
///
/// # Returns
///
/// A `Result` containing the `Magnet` data or an error string.
pub fn parse_magnet_uri(uri: &str) -> Result<Magnet, String> {
    let url = Url::parse(uri).map_err(|e| format!("Failed to parse magnet URI: {}", e))?;

    if url.scheme() != "magnet" {
        return Err("Not a magnet URI".to_string());
    }

    let mut info_hash = None;
    let mut display_name = None;
    let mut trackers = Vec::new();

    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "xt" => {
                if value.starts_with("urn:btih:") {
                    let hash_str = &value[9..];
                    let hash = hex::decode(hash_str)
                        .map_err(|e| format!("Invalid info hash hex: {}", e))?;
                    if hash.len() == 20 {
                        info_hash = Some(hash);
                    } else {
                        return Err(format!("Info hash has incorrect length: {}", hash.len()));
                    }
                }
            }
            "dn" => display_name = Some(value.to_string()),
            "tr" => trackers.push(value.to_string()),
            _ => {} // Ignore other parameters
        }
    }

    let info_hash = info_hash.ok_or("Magnet URI is missing the info hash (xt parameter)")?;

    Ok(Magnet {
        display_name,
        info_hash,
        trackers,
    })
}