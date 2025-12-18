// Interactive REPL mode for easier testing and server deployment
use crate::dht::{models::FileMetadata, DhtService};
use crate::ethereum::GethProcess;
use crate::file_transfer::{AttemptStatus, FileTransferService};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::sync::Arc;
use tracing::error;

pub struct ReplContext {
    pub dht_service: Arc<DhtService>,
    pub file_transfer_service: Option<Arc<FileTransferService>>,
    pub geth_process: Option<GethProcess>,
    pub peer_id: String,
}

pub async fn run_repl(context: ReplContext) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n┌─────────────────────────────────────────────────────────┐");
    println!("│  Chiral Network v0.1.0 - Interactive Shell             │");
    println!("│  Type 'help' for commands, 'quit' to exit              │");
    println!("└─────────────────────────────────────────────────────────┘");
    println!("\nPeer ID: {}", context.peer_id);
    println!();

    let mut rl = DefaultEditor::new()?;

    // Load command history if it exists
    let history_file = std::env::var("HOME")
        .ok()
        .map(|h| std::path::PathBuf::from(h).join(".chiral_history"))
        .unwrap_or_else(|| std::path::PathBuf::from(".chiral_history"));

    let _ = rl.load_history(&history_file);

    loop {
        let readline = rl.readline("chiral> ");
        match readline {
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                rl.add_history_entry(line)?;

                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.is_empty() {
                    continue;
                }

                let command = parts[0];
                let args = &parts[1..];

                match handle_command(command, args, &context).await {
                    Ok(should_exit) => {
                        if should_exit {
                            break;
                        }
                    }
                    Err(e) => {
                        eprintln!("❌ Error: {}", e);
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("^C");
                continue;
            }
            Err(ReadlineError::Eof) => {
                println!("exit");
                break;
            }
            Err(err) => {
                error!("Readline error: {:?}", err);
                break;
            }
        }
    }

    // Save command history
    let _ = rl.save_history(&history_file);
    println!("Shutting down gracefully...");
    Ok(())
}

async fn handle_command(
    command: &str,
    args: &[&str],
    context: &ReplContext,
) -> Result<bool, String> {
    match command {
        "help" | "h" | "?" => {
            print_help();
            Ok(false)
        }
        "quit" | "exit" | "q" => {
            Ok(true)
        }
        "status" | "s" => {
            cmd_status(context).await?;
            Ok(false)
        }
        "peers" => {
            cmd_peers(args, context).await?;
            Ok(false)
        }
        "list" | "ls" => {
            cmd_list(args, context).await?;
            Ok(false)
        }
        "add" => {
            cmd_add(args, context).await?;
            Ok(false)
        }
        "download" | "dl" => {
            cmd_download(args, context).await?;
            Ok(false)
        }
        "dht" => {
            cmd_dht(args, context).await?;
            Ok(false)
        }
        "mining" | "mine" => {
            cmd_mining(args, context).await?;
            Ok(false)
        }
        "clear" | "cls" => {
            print!("\x1B[2J\x1B[1;1H");
            Ok(false)
        }
        _ => {
            println!("❌ Unknown command: '{}'", command);
            println!("   Type 'help' for available commands");
            Ok(false)
        }
    }
}

fn print_help() {
    println!("\n📚 Available Commands:");
    println!("  ┌─────────────────────────────────────────────────────────────┐");
    println!("  │ General                                                     │");
    println!("  ├─────────────────────────────────────────────────────────────┤");
    println!("  │  help, h, ?              Show this help message             │");
    println!("  │  status, s               Show network status                │");
    println!("  │  clear, cls              Clear screen                       │");
    println!("  │  quit, exit, q           Exit REPL                          │");
    println!("  ├─────────────────────────────────────────────────────────────┤");
    println!("  │ Network                                                     │");
    println!("  ├─────────────────────────────────────────────────────────────┤");
    println!("  │  peers [count|list]      Show peer information              │");
    println!("  │  dht [status|get <hash>] DHT operations                     │");
    println!("  ├─────────────────────────────────────────────────────────────┤");
    println!("  │ Files                                                       │");
    println!("  ├─────────────────────────────────────────────────────────────┤");
    println!("  │  list [files|downloads]  List files or active downloads     │");
    println!("  │  add <path>              Add file to share                  │");
    println!("  │  download <hash>         Download file by hash              │");
    println!("  ├─────────────────────────────────────────────────────────────┤");
    println!("  │ Mining                                                      │");
    println!("  ├─────────────────────────────────────────────────────────────┤");
    println!("  │  mining status           Show mining status                 │");
    println!("  │  mining start [threads]  Start mining (requires geth)       │");
    println!("  │  mining stop             Stop mining                        │");
    println!("  └─────────────────────────────────────────────────────────────┘");
    println!();
}

async fn cmd_status(context: &ReplContext) -> Result<(), String> {
    println!("\n📊 Network Status:");
    println!("  ┌─────────────────────────────────────────────────────────────┐");

    // Get connected peers
    let connected_peers = context.dht_service.get_connected_peers().await;
    println!("  │ Connected Peers: {:>42} │", connected_peers.len());

    // Get DHT metrics
    let metrics = context.dht_service.metrics_snapshot().await;
    println!("  │ Reachability: {:>45} │", format!("{:?}", metrics.reachability));
    println!("  │ NAT Status: {:>47} │",
        if metrics.observed_addrs.is_empty() { "Unknown" } else { "Active" });

    // AutoNAT status
    println!("  │ AutoNAT: {:>50} │",
        if metrics.autonat_enabled { "Enabled" } else { "Disabled" });

    // Relay status
    let relay_status = if metrics.active_relay_peer_id.is_some() {
        format!("Active ({})", metrics.active_relay_peer_id.as_ref().unwrap())
    } else {
        "None".to_string()
    };
    println!("  │ Circuit Relay: {:>44} │", relay_status);

    // DCUtR stats
    if metrics.dcutr_enabled {
        let success_rate = if metrics.dcutr_hole_punch_attempts > 0 {
            (metrics.dcutr_hole_punch_successes as f64 / metrics.dcutr_hole_punch_attempts as f64) * 100.0
        } else {
            0.0
        };
        println!("  │ DCUtR Success Rate: {:>38} │",
            format!("{:.1}% ({}/{})", success_rate,
                metrics.dcutr_hole_punch_successes,
                metrics.dcutr_hole_punch_attempts));
    }

    // File transfer stats
    if let Some(ft) = &context.file_transfer_service {
        let snapshot = ft.download_metrics_snapshot().await;
        println!("  ├─────────────────────────────────────────────────────────────┤");
        println!("  │ Download Stats:                                             │");
        println!("  │   Success: {:>48} │", snapshot.total_success);
        println!("  │   Failures: {:>47} │", snapshot.total_failures);
        println!("  │   Retries: {:>48} │", snapshot.total_retries);
    }

    println!("  └─────────────────────────────────────────────────────────────┘");
    println!();

    Ok(())
}

async fn cmd_peers(args: &[&str], context: &ReplContext) -> Result<(), String> {
    let subcommand = args.get(0).unwrap_or(&"count");

    match *subcommand {
        "count" => {
            let connected_peers = context.dht_service.get_connected_peers().await;
            println!("\n📡 Connected peers: {}", connected_peers.len());
            println!();
        }
        "list" => {
            let connected_peers = context.dht_service.get_connected_peers().await;

            if connected_peers.is_empty() {
                println!("\n📡 No connected peers");
                println!();
                return Ok(());
            }

            println!("\n📡 Connected Peers:");
            println!("  ┌────────────────────────────────────────────────────────┐");

            for (i, peer) in connected_peers.iter().take(20).enumerate() {
                let peer_short = if peer.len() > 20 {
                    format!("{}...{}", &peer[..8], &peer[peer.len()-8..])
                } else {
                    peer.clone()
                };
                println!("  │ {:>2}. {:<50} │", i + 1, peer_short);
            }

            if connected_peers.len() > 20 {
                println!("  │ ... and {} more", connected_peers.len() - 20);
            }

            println!("  └────────────────────────────────────────────────────────┘");
            println!();
        }
        _ => {
            return Err(format!("Unknown peers subcommand: '{}'. Use 'count' or 'list'", subcommand));
        }
    }

    Ok(())
}

async fn cmd_list(args: &[&str], context: &ReplContext) -> Result<(), String> {
    let what = args.get(0).unwrap_or(&"files");

    match *what {
        "files" | "seeding" => {
            println!("\n📤 Seeding Files:");
            println!("  (This feature requires integration with file storage service)");
            println!();
        }
        "downloads" | "dl" => {
            if let Some(ft) = &context.file_transfer_service {
                let snapshot = ft.download_metrics_snapshot().await;

                if snapshot.recent_attempts.is_empty() {
                    println!("\n📥 No recent downloads");
                    println!();
                    return Ok(());
                }

                println!("\n📥 Recent Downloads:");
                println!("  ┌────────────────────────────────────────────────────────┐");

                for attempt in snapshot.recent_attempts.iter().take(10) {
                    let hash_short = if attempt.file_hash.len() > 16 {
                        format!("{}...{}", &attempt.file_hash[..8], &attempt.file_hash[attempt.file_hash.len()-4..])
                    } else {
                        attempt.file_hash.clone()
                    };

                    let status_icon = match attempt.status {
                        AttemptStatus::Success => "✓",
                        AttemptStatus::Failed => "✗",
                        AttemptStatus::Retrying => "◷",
                    };

                    println!("  │ {} {} (attempt {}/{})         │",
                        status_icon, hash_short, attempt.attempt, attempt.max_attempts);
                }

                println!("  └────────────────────────────────────────────────────────┘");
                println!();
            } else {
                println!("\n📥 File transfer service not available");
                println!();
            }
        }
        _ => {
            return Err(format!("Unknown list target: '{}'. Use 'files' or 'downloads'", what));
        }
    }

    Ok(())
}

async fn cmd_add(args: &[&str], context: &ReplContext) -> Result<(), String> {
    if args.is_empty() {
        return Err("Usage: add <file_path>".to_string());
    }

    let file_path = args.join(" ");

    // Check if file exists
    if !std::path::Path::new(&file_path).exists() {
        return Err(format!("File not found: {}", file_path));
    }

    // Read file
    let file_data = std::fs::read(&file_path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    // Calculate hash
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(&file_data);
    let hash = format!("Qm{:x}", hasher.finalize());

    let file_name = std::path::Path::new(&file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Create metadata
    let metadata = FileMetadata {
        merkle_root: hash.clone(),
        file_name: file_name.clone(),
        file_size: file_data.len() as u64,
        file_data: file_data.clone(),
        seeders: vec![context.peer_id.clone()],
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        mime_type: None,
        is_encrypted: false,
        encryption_method: None,
        key_fingerprint: None,
        parent_hash: None,
        cids: None,
        is_root: true,
        encrypted_key_bundle: None,
        download_path: None,
        price: 0.0,
        uploader_address: None,
        ftp_sources: None,
        http_sources: None,
        info_hash: None,
        trackers: None,
        ed2k_sources: None,
    };

    // Publish to DHT
    context.dht_service.publish_file(metadata, None).await
        .map_err(|e| format!("Failed to publish file: {}", e))?;

    println!("\n✓ Added and seeding: {} ({})", file_name, hash);
    println!("  Size: {} bytes", file_data.len());
    println!();

    Ok(())
}

async fn cmd_download(args: &[&str], context: &ReplContext) -> Result<(), String> {
    if args.is_empty() {
        return Err("Usage: download <file_hash>".to_string());
    }

    let hash = args[0];

    println!("\n📥 Searching for file: {}", hash);

    // Try to search file in DHT
    match context.dht_service.get_file(hash.to_string()).await {
        Ok(_) => {
            println!("✓ Search initiated for: {}", hash);
            println!("  (Full download implementation requires file transfer service integration)");
            println!();
        }
        Err(e) => {
            return Err(format!("Failed to search DHT: {}", e));
        }
    }

    Ok(())
}

async fn cmd_dht(args: &[&str], context: &ReplContext) -> Result<(), String> {
    if args.is_empty() {
        return Err("Usage: dht <status|get <hash>>".to_string());
    }

    match args[0] {
        "status" => {
            let metrics = context.dht_service.metrics_snapshot().await;

            println!("\n🔍 DHT Status:");
            println!("  ┌─────────────────────────────────────────────────────────┐");
            println!("  │ Reachability: {:>43} │", format!("{:?}", metrics.reachability));
            println!("  │ Confidence: {:>45} │", format!("{:?}", metrics.reachability_confidence));

            if !metrics.observed_addrs.is_empty() {
                println!("  ├─────────────────────────────────────────────────────────┤");
                println!("  │ Observed Addresses:                                     │");
                for addr in metrics.observed_addrs.iter().take(3) {
                    let addr_str = if addr.len() > 50 {
                        format!("{}...", &addr[..47])
                    } else {
                        addr.clone()
                    };
                    println!("  │   {:<53} │", addr_str);
                }
            }

            println!("  └─────────────────────────────────────────────────────────┘");
            println!();
        }
        "get" => {
            if args.len() < 2 {
                return Err("Usage: dht get <hash>".to_string());
            }

            let hash = args[1];
            println!("\n🔍 Searching DHT for: {}", hash);

            match context.dht_service.get_file(hash.to_string()).await {
                Ok(_) => {
                    println!("✓ DHT search initiated for: {}", hash);
                    println!("  Check logs for results");
                    println!();
                }
                Err(e) => {
                    return Err(format!("Search failed: {}", e));
                }
            }
        }
        _ => {
            return Err(format!("Unknown dht subcommand: '{}'", args[0]));
        }
    }

    Ok(())
}

async fn cmd_mining(args: &[&str], context: &ReplContext) -> Result<(), String> {
    if args.is_empty() {
        return Err("Usage: mining <status|start|stop>".to_string());
    }

    if context.geth_process.is_none() {
        return Err("Mining requires geth. Start with --enable-geth flag".to_string());
    }

    match args[0] {
        "status" => {
            println!("\n⛏️  Mining Status:");
            println!("  (Mining status requires geth integration)");
            println!();
        }
        "start" => {
            let threads = args.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(1);
            println!("\n⛏️  Starting mining with {} thread(s)...", threads);
            println!("  (Mining start requires geth integration)");
            println!();
        }
        "stop" => {
            println!("\n⛏️  Stopping mining...");
            println!("  (Mining stop requires geth integration)");
            println!();
        }
        _ => {
            return Err(format!("Unknown mining subcommand: '{}'", args[0]));
        }
    }

    Ok(())
}
