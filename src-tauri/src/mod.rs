use crate::protocols::ProtocolHandler;
use async_trait::async_trait;
use bt_client::Torrent;
use futures::stream::StreamExt;
use std::env;
use std::path::Path;

pub struct BitTorrentHandler;

#[async_trait]
impl ProtocolHandler for BitTorrentHandler {
    async fn download(&self, identifier: &str) -> Result<(), String> {
        let torrent = if identifier.starts_with("magnet:") {
            println!("Parsing magnet link: {}", identifier);
            Torrent::from_magnet(identifier)
                .await
                .map_err(|e| format!("Failed to parse magnet link: {}", e))?
        } else if Path::new(identifier).exists() && identifier.ends_with(".torrent") {
            println!("Parsing .torrent file: {}", identifier);
            Torrent::from_file(identifier)
                .await
                .map_err(|e| format!("Failed to parse .torrent file: {}", e))?
        } else {
            return Err(format!(
                "Invalid identifier: '{}' is not a magnet link or a valid .torrent file path.",
                identifier
            ));
        };

        println!("Successfully parsed torrent.");
        println!("  Info Hash: {}", torrent.info_hash_hex());
        println!("  Display Name: {}", torrent.display_name());
        println!("  Trackers: {:?}", torrent.trackers());
        if !torrent.files().is_empty() {
            println!("  Files:");
            for file in torrent.files() {
                println!("    - {} ({} bytes)", file.path.to_string_lossy(), file.length);
            }
        }
        let output_dir = env::current_dir().unwrap().join("downloads");
        tokio::fs::create_dir_all(&output_dir).await.unwrap();
        println!("Downloading to: {}", output_dir.to_string_lossy());

        let (mut download_stream, download_handle) = torrent
            .download_to_directory(output_dir)
            .await
            .map_err(|e| format!("Failed to start download: {}", e))?;

        while let Some(piece) = download_stream.next().await {
            println!(
                "Downloaded piece {}/{}",
                piece.index + 1,
                torrent.piece_count()
            );
        }

        println!("Download finished, waiting for finalization...");
        download_handle
            .await
            .map_err(|e| format!("Error during download finalization: {}", e))?;

        println!("Download completed successfully!");

        Ok(())
    }

    async fn seed(&self, file_path: &str) -> Result<String, String> {
        let file_path_obj = Path::new(file_path);
        println!("Seeding file via BitTorrent: {}", file_path_obj.display());
        if !file_path_obj.exists() {
            return Err(format!("File not found: {}", file_path));
        }

        // The directory where the file to be seeded is located.
        // This will also be the directory where the torrent is "downloaded" to,
        // which in the case of seeding, means it will verify the existing files.
        let seed_dir = file_path_obj.parent().ok_or_else(|| "Invalid file path: cannot determine parent directory".to_string())?;

        println!("Creating torrent for: {}", file_path_obj.display());
        let torrent = Torrent::from_file_path(file_path_obj)
            .await
            .map_err(|e| format!("Failed to create torrent from file: {}", e))?;

        let magnet_link = torrent.magnet_link();
        println!("Generated magnet link: {}", magnet_link);
        println!("Starting seeding process... Files will be verified first.");

        // Start a "download" which will verify the files and then begin seeding.
        let (_download_stream, download_handle) = torrent
            .download_to_directory(seed_dir)
            .await
            .map_err(|e| format!("Failed to start seeding: {}", e))?;

        // Spawn a task to keep the seeding process running in the background.
        tokio::spawn(download_handle);

        Ok(magnet_link)
    }
}
