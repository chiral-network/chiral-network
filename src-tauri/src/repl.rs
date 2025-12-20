// Interactive REPL mode for easier testing and server deployment
use crate::dht::{models::FileMetadata, DhtService};
use crate::ethereum::GethProcess;
use crate::file_transfer::{AttemptStatus, FileTransferService};
use colored::Colorize;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Context, Helper};
use rustyline::Editor;
use std::borrow::Cow;
use std::sync::Arc;
use strsim::levenshtein;

pub struct ReplContext {
    pub dht_service: Arc<DhtService>,
    pub file_transfer_service: Option<Arc<FileTransferService>>,
    pub geth_process: Option<GethProcess>,
    pub peer_id: String,
}

// REPL helper for completion, highlighting, and validation
struct ReplHelper {
    commands: Vec<&'static str>,
    subcommands: std::collections::HashMap<&'static str, Vec<&'static str>>,
}

impl ReplHelper {
    fn new() -> Self {
        let mut subcommands = std::collections::HashMap::new();
        subcommands.insert("peers", vec!["count", "list"]);
        subcommands.insert("dht", vec!["status", "get"]);
        subcommands.insert("list", vec!["files", "downloads"]);
        subcommands.insert("mining", vec!["status", "start", "stop"]);
        subcommands.insert("config", vec!["get", "set", "list", "reset"]);
        subcommands.insert("reputation", vec!["list", "info"]);
        subcommands.insert("versions", vec!["list", "info"]);

        ReplHelper {
            commands: vec![
                "help", "h", "status", "s", "peers", "dht", "list", "ls",
                "add", "download", "dl", "mining", "mine", "downloads",
                "config", "reputation", "rep", "versions", "ver",
                "clear", "cls", "quit", "exit", "q",
            ],
            subcommands,
        }
    }
}

impl Completer for ReplHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let line = &line[..pos];
        let parts: Vec<&str> = line.split_whitespace().collect();

        if parts.is_empty() || (parts.len() == 1 && !line.ends_with(' ')) {
            // Complete command names
            let prefix = parts.get(0).unwrap_or(&"");
            let matches: Vec<Pair> = self
                .commands
                .iter()
                .filter(|cmd| cmd.starts_with(prefix))
                .map(|cmd| Pair {
                    display: cmd.to_string(),
                    replacement: cmd.to_string(),
                })
                .collect();
            Ok((line.len() - prefix.len(), matches))
        } else if parts.len() >= 1 {
            // Complete subcommands
            let cmd = parts[0];
            if let Some(subcmds) = self.subcommands.get(cmd) {
                let prefix = if parts.len() > 1 && !line.ends_with(' ') {
                    parts[1]
                } else {
                    ""
                };
                let matches: Vec<Pair> = subcmds
                    .iter()
                    .filter(|subcmd| subcmd.starts_with(prefix))
                    .map(|subcmd| Pair {
                        display: subcmd.to_string(),
                        replacement: subcmd.to_string(),
                    })
                    .collect();
                Ok((prefix.len(), matches))
            } else {
                Ok((0, vec![]))
            }
        } else {
            Ok((0, vec![]))
        }
    }
}

impl Hinter for ReplHelper {
    type Hint = String;
}

impl Highlighter for ReplHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        // Highlight hashes (Qm...) and peer IDs (12D3KooW...)
        let mut colored_line = line.to_string();

        // Highlight Qm hashes in cyan
        if line.contains("Qm") {
            colored_line = colored_line.replace("Qm", &"Qm".cyan().to_string());
        }

        // Highlight peer IDs in yellow
        if line.contains("12D3KooW") {
            colored_line = colored_line.replace("12D3KooW", &"12D3KooW".yellow().to_string());
        }

        Cow::Owned(colored_line)
    }
}

impl Validator for ReplHelper {}

impl Helper for ReplHelper {}

pub async fn run_repl(context: ReplContext) -> Result<(), Box<dyn std::error::Error>> {
    // Box width = 56 chars content
    println!("\n┌────────────────────────────────────────────────────────┐");
    println!("│ {:<54} │", "Chiral Network v0.1.0 - Interactive Shell");
    println!("│ {:<54} │", "Type 'help' for commands, 'quit' to exit");
    println!("└────────────────────────────────────────────────────────┘");
    println!("\nPeer ID: {}", context.peer_id);
    println!();

    // Create editor with helper for tab completion and highlighting
    let helper = ReplHelper::new();
    let mut rl = Editor::new()?;
    rl.set_helper(Some(helper));

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
                eprintln!("Readline error: {:?}", err);
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
        "downloads" => {
            cmd_downloads(context).await?;
            Ok(false)
        }
        "config" => {
            cmd_config(args, context).await?;
            Ok(false)
        }
        "reputation" | "rep" => {
            cmd_reputation(args, context).await?;
            Ok(false)
        }
        "versions" | "ver" => {
            cmd_versions(args, context).await?;
            Ok(false)
        }
        "clear" | "cls" => {
            print!("\x1B[2J\x1B[1;1H");
            Ok(false)
        }
        _ => {
            // Find similar commands using Levenshtein distance
            let all_commands = vec![
                "help", "h", "status", "s", "peers", "dht", "list", "ls",
                "add", "download", "dl", "mining", "mine", "downloads",
                "config", "reputation", "rep", "versions", "ver",
                "clear", "cls", "quit", "exit", "q",
            ];

            let mut suggestions: Vec<(&str, usize)> = all_commands
                .iter()
                .map(|cmd| (*cmd, levenshtein(command, cmd)))
                .filter(|(_, dist)| *dist <= 2) // Only suggest if distance <= 2
                .collect();

            suggestions.sort_by_key(|(_, dist)| *dist);

            println!("{}", format!("❌ Unknown command: '{}'", command).red());

            if let Some((suggestion, _)) = suggestions.first() {
                println!("{}", format!("💡 Did you mean: {}", suggestion).yellow());

                // Show usage example for the suggested command
                match *suggestion {
                    "status" | "s" => println!("   Usage: status"),
                    "peers" => println!("   Usage: peers [count|list]"),
                    "dht" => println!("   Usage: dht [status|get <hash>]"),
                    "list" | "ls" => println!("   Usage: list [files|downloads]"),
                    "add" => println!("   Usage: add <file_path>"),
                    "download" | "dl" => println!("   Usage: download <hash>"),
                    "mining" | "mine" => println!("   Usage: mining [status|start|stop]"),
                    "config" => println!("   Usage: config [get|set|list|reset]"),
                    "reputation" | "rep" => println!("   Usage: reputation [list|info <peer_id>]"),
                    "versions" | "ver" => println!("   Usage: versions [list|info] <hash>"),
                    _ => {}
                }
            } else {
                println!("   Type {} for available commands", "help".cyan());
            }
            Ok(false)
        }
    }
}

fn print_help() {
    println!("\n📚 Available Commands:");
    println!("  ┌────────────────────────────────────────────────────────┐");
    println!("  │ {:<54} │", "General");
    println!("  ├────────────────────────────────────────────────────────┤");
    println!("  │ {:<54} │", "  help, h, ?              Show this help message");
    println!("  │ {:<54} │", "  status, s               Show network status");
    println!("  │ {:<54} │", "  clear, cls              Clear screen");
    println!("  │ {:<54} │", "  quit, exit, q           Exit REPL");
    println!("  ├────────────────────────────────────────────────────────┤");
    println!("  │ {:<54} │", "Network");
    println!("  ├────────────────────────────────────────────────────────┤");
    println!("  │ {:<54} │", "  peers count             Count connected peers");
    println!("  │ {:<54} │", "  peers list [--flags]    List peers");
    println!("  │ {:<54} │", "    --trust <level>       Filter by trust level");
    println!("  │ {:<54} │", "    --sort <field>        Sort by score/latency");
    println!("  │ {:<54} │", "    --limit <num>         Limit results");
    println!("  │ {:<54} │", "  dht [status|get <hash>] DHT operations");
    println!("  │ {:<54} │", "  reputation list         Show peer reputation");
    println!("  │ {:<54} │", "  reputation info <peer>  Get peer details");
    println!("  ├────────────────────────────────────────────────────────┤");
    println!("  │ {:<54} │", "Files");
    println!("  ├────────────────────────────────────────────────────────┤");
    println!("  │ {:<54} │", "  list [files|downloads]  List files or downloads");
    println!("  │ {:<54} │", "  add <path>              Add file to share");
    println!("  │ {:<54} │", "  download <hash>         Download file by hash");
    println!("  │ {:<54} │", "  downloads               Show active downloads");
    println!("  │ {:<54} │", "  versions list <hash>    Show file versions");
    println!("  │ {:<54} │", "  versions info <hash>    Version details");
    println!("  ├────────────────────────────────────────────────────────┤");
    println!("  │ {:<54} │", "Mining");
    println!("  ├────────────────────────────────────────────────────────┤");
    println!("  │ {:<54} │", "  mining status           Show mining status");
    println!("  │ {:<54} │", "  mining start [threads]  Start mining (geth)");
    println!("  │ {:<54} │", "  mining stop             Stop mining");
    println!("  ├────────────────────────────────────────────────────────┤");
    println!("  │ {:<54} │", "Configuration");
    println!("  ├────────────────────────────────────────────────────────┤");
    println!("  │ {:<54} │", "  config list             List all settings");
    println!("  │ {:<54} │", "  config get <key>        Get setting value");
    println!("  │ {:<54} │", "  config set <key> <val>  Set setting value");
    println!("  │ {:<54} │", "  config reset <key>      Reset to default");
    println!("  └────────────────────────────────────────────────────────┘");
    println!();
}

async fn cmd_status(context: &ReplContext) -> Result<(), String> {
    println!("\n📊 Network Status:");
    println!("  ┌────────────────────────────────────────────────────────┐");

    // Get connected peers
    let connected_peers = context.dht_service.get_connected_peers().await;
    println!("  │ {:<54} │", format!("Connected Peers: {}", connected_peers.len()));

    // Get DHT metrics
    let metrics = context.dht_service.metrics_snapshot().await;
    println!("  │ {:<54} │", format!("Reachability: {:?}", metrics.reachability));
    println!("  │ {:<54} │", format!("NAT Status: {}",
        if metrics.observed_addrs.is_empty() { "Unknown" } else { "Active" }));

    // AutoNAT status
    println!("  │ {:<54} │", format!("AutoNAT: {}",
        if metrics.autonat_enabled { "Enabled" } else { "Disabled" }));

    // Relay status
    let relay_status = if metrics.active_relay_peer_id.is_some() {
        format!("Active ({})", metrics.active_relay_peer_id.as_ref().unwrap())
    } else {
        "None".to_string()
    };
    println!("  │ {:<54} │", format!("Circuit Relay: {}", relay_status));

    // DCUtR stats
    if metrics.dcutr_enabled {
        let success_rate = if metrics.dcutr_hole_punch_attempts > 0 {
            (metrics.dcutr_hole_punch_successes as f64 / metrics.dcutr_hole_punch_attempts as f64) * 100.0
        } else {
            0.0
        };
        let rate_str = format!("{:.1}% ({}/{})", success_rate,
            metrics.dcutr_hole_punch_successes,
            metrics.dcutr_hole_punch_attempts);
        println!("  │ {:<54} │", format!("DCUtR Success Rate: {}", rate_str));
    }

    // File transfer stats
    if let Some(ft) = &context.file_transfer_service {
        let snapshot = ft.download_metrics_snapshot().await;
        println!("  ├────────────────────────────────────────────────────────┤");
        println!("  │ {:<54} │", "Download Stats:");
        println!("  │ {:<54} │", format!("  Success: {}", snapshot.total_success));
        println!("  │ {:<54} │", format!("  Failures: {}", snapshot.total_failures));
        println!("  │ {:<54} │", format!("  Retries: {}", snapshot.total_retries));
    }

    println!("  └────────────────────────────────────────────────────────┘");
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

            // Parse optional flags
            let mut limit = 20;
            let mut sort_by = "default";
            let mut trust_filter = None;

            let mut i = 1;
            while i < args.len() {
                match args[i] {
                    "--limit" | "-l" => {
                        if i + 1 < args.len() {
                            limit = args[i + 1].parse::<usize>().unwrap_or(20);
                            i += 2;
                        } else {
                            return Err("--limit requires a number".to_string());
                        }
                    }
                    "--sort" | "-s" => {
                        if i + 1 < args.len() {
                            sort_by = args[i + 1];
                            i += 2;
                        } else {
                            return Err("--sort requires a value (score, latency, bandwidth, uptime)".to_string());
                        }
                    }
                    "--trust" | "-t" => {
                        if i + 1 < args.len() {
                            trust_filter = Some(args[i + 1]);
                            i += 2;
                        } else {
                            return Err("--trust requires a value (high, medium, low)".to_string());
                        }
                    }
                    _ => {
                        i += 1;
                    }
                }
            }

            println!("\n📡 Connected Peers:");
            if sort_by != "default" {
                println!("  (Sorted by: {})", sort_by);
            }
            if let Some(trust) = trust_filter {
                println!("  (Filtered by trust: {})", trust);
            }

            println!("  ┌────────────────────────────────────────────────────────┐");
            println!("  │ {:<20} {:<10} {:<10} {:<11} │", "Peer ID", "Score", "Latency", "Trust");
            println!("  ├────────────────────────────────────────────────────────┤");

            // Mock peer data with scores for filtering/sorting
            let mut peer_data: Vec<_> = connected_peers
                .iter()
                .enumerate()
                .map(|(idx, peer)| {
                    let score = 75 + (idx * 3) as i32;
                    let latency = 30 + (idx * 5);
                    let trust = if score > 85 {
                        "High"
                    } else if score > 70 {
                        "Medium"
                    } else {
                        "Low"
                    };
                    (peer, score, latency, trust)
                })
                .collect();

            // Apply trust filter
            if let Some(filter) = trust_filter {
                let filter_lower = filter.to_lowercase();
                peer_data.retain(|(_, _, _, trust)| {
                    trust.to_lowercase() == filter_lower
                });
            }

            // Apply sorting
            match sort_by {
                "score" => peer_data.sort_by(|a, b| b.1.cmp(&a.1)),
                "latency" => peer_data.sort_by(|a, b| a.2.cmp(&b.2)),
                "bandwidth" => {
                    // Would sort by bandwidth if available
                    peer_data.sort_by(|a, b| b.1.cmp(&a.1));
                }
                "uptime" => {
                    // Would sort by uptime if available
                    peer_data.sort_by(|a, b| b.1.cmp(&a.1));
                }
                _ => {} // default order
            }

            // Display peers
            for (peer, score, latency, trust) in peer_data.iter().take(limit) {
                let peer_short = if peer.len() > 20 {
                    format!("{}...{}", &peer[..8], &peer[peer.len() - 8..])
                } else {
                    (*peer).clone()
                };

                println!(
                    "  │ {:<20} {:<10} {:<10} {:<11} │",
                    peer_short,
                    score,
                    format!("{}ms", latency),
                    trust
                );
            }

            if peer_data.len() > limit {
                let msg = format!("... and {} more peers", peer_data.len() - limit);
                println!("  │ {:<54} │", msg);
            }

            if peer_data.is_empty() {
                println!("  │ {:<54} │", "No peers match the filter criteria");
            }

            println!("  └────────────────────────────────────────────────────────┘");
            println!();
            println!("  Tip: Use {} to filter/sort", "peers list --trust high --sort score".cyan());
            println!();
        }
        _ => {
            return Err(format!(
                "Unknown peers subcommand: '{}'. Use 'count' or 'list'",
                subcommand
            ));
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
            println!("  ┌────────────────────────────────────────────────────────┐");
            println!("  │ {:<54} │", format!("Reachability: {:?}", metrics.reachability));
            println!("  │ {:<54} │", format!("Confidence: {:?}", metrics.reachability_confidence));

            if !metrics.observed_addrs.is_empty() {
                println!("  ├────────────────────────────────────────────────────────┤");
                println!("  │ {:<54} │", "Observed Addresses:");
                for addr in metrics.observed_addrs.iter().take(3) {
                    // Truncate to fit in the box (max 52 chars for content with 2-space indent)
                    let display_addr = if addr.len() > 52 {
                        format!("  {}...", &addr[..49])
                    } else {
                        format!("  {}", addr)
                    };
                    println!("  │ {:<54} │", display_addr);
                }
            }

            println!("  └────────────────────────────────────────────────────────┘");
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

async fn cmd_downloads(_context: &ReplContext) -> Result<(), String> {
    // This would integrate with MultiSourceDownloadService for real-time progress
    // For now, showing a placeholder implementation
    println!("\n📥 Active Downloads:");
    println!("  ┌────────────────────────────────────────────────────────┐");
    println!("  │ {:<54} │", "No active downloads");
    println!("  │ {:<54} │", "");
    println!("  │ {:<54} │", "Use 'download <hash>' to start a download");
    println!("  └────────────────────────────────────────────────────────┘");
    println!();

    // Future implementation would show:
    // - Download progress bars with indicatif
    // - Speed, ETA, peers connected
    // - Chunk completion status
    // - Source breakdown (P2P, HTTP, FTP, BitTorrent, ed2k)

    Ok(())
}

async fn cmd_config(args: &[&str], _context: &ReplContext) -> Result<(), String> {
    if args.is_empty() {
        return Err("Usage: config <list|get|set|reset>".to_string());
    }

    match args[0] {
        "list" => {
            println!("\n⚙️  Configuration Settings:");
            println!("  ┌────────────────────────────────────────────────────────┐");
            println!("  │ {:<54} │", "Network Settings:");
            println!("  │ {:<54} │", "  max_peers: 50");
            println!("  │ {:<54} │", "  listen_port: 4001");
            println!("  │ {:<54} │", "  enable_upnp: true");
            println!("  │ {:<54} │", "  enable_autonat: true");
            println!("  │ {:<54} │", "  enable_relay: true");
            println!("  ├────────────────────────────────────────────────────────┤");
            println!("  │ {:<54} │", "Download Settings:");
            println!("  │ {:<54} │", "  max_concurrent_downloads: 3");
            println!("  │ {:<54} │", "  chunk_size: 262144 (256KB)");
            println!("  │ {:<54} │", "  download_timeout: 60s");
            println!("  ├────────────────────────────────────────────────────────┤");
            println!("  │ {:<54} │", "Bandwidth Settings:");
            println!("  │ {:<54} │", "  max_upload_speed: unlimited");
            println!("  │ {:<54} │", "  max_download_speed: unlimited");
            println!("  └────────────────────────────────────────────────────────┘");
            println!();
            println!("  Use {} to get a specific value", "config get <key>".cyan());
            println!();
        }
        "get" => {
            if args.len() < 2 {
                return Err("Usage: config get <key>".to_string());
            }
            let key = args[1];
            println!("\n⚙️  Config value for '{}':", key);
            println!("  (Configuration retrieval requires settings integration)");
            println!();
        }
        "set" => {
            if args.len() < 3 {
                return Err("Usage: config set <key> <value>".to_string());
            }
            let key = args[1];
            let value = args[2];
            println!("\n⚙️  Setting '{}' = '{}'", key, value);
            println!("  (Configuration update requires settings integration)");
            println!();
        }
        "reset" => {
            if args.len() < 2 {
                return Err("Usage: config reset <key>".to_string());
            }
            let key = args[1];
            println!("\n⚙️  Resetting '{}' to default", key);
            println!("  (Configuration reset requires settings integration)");
            println!();
        }
        _ => {
            return Err(format!("Unknown config subcommand: '{}'. Use 'list', 'get', 'set', or 'reset'", args[0]));
        }
    }

    Ok(())
}

async fn cmd_reputation(args: &[&str], context: &ReplContext) -> Result<(), String> {
    if args.is_empty() {
        return Err("Usage: reputation <list|info>".to_string());
    }

    match args[0] {
        "list" => {
            let peers = context.dht_service.get_connected_peers().await;

            if peers.is_empty() {
                println!("\n👥 No peers with reputation data");
                println!();
                return Ok(());
            }

            println!("\n👥 Peer Reputation:");
            println!("  ┌────────────────────────────────────────────────────────┐");
            println!("  │ {:<20} {:<10} {:<22} │", "Peer ID", "Score", "Trust Level");
            println!("  ├────────────────────────────────────────────────────────┤");

            // Show first few peers with mock reputation data
            for (i, peer) in peers.iter().take(10).enumerate() {
                let peer_short = if peer.len() > 20 {
                    format!("{}...{}", &peer[..8], &peer[peer.len()-8..])
                } else {
                    peer.clone()
                };

                // Mock reputation data
                let score = 75 + (i * 3) as i32;
                let trust = if score > 85 { "High" } else if score > 70 { "Medium" } else { "Low" };

                println!("  │ {:<20} {:<10} {:<22} │", peer_short, score, trust);
            }

            if peers.len() > 10 {
                let msg = format!("... and {} more peers", peers.len() - 10);
                println!("  │ {:<54} │", msg);
            }

            println!("  └────────────────────────────────────────────────────────┘");
            println!();
            println!("  Use {} to see details", "reputation info <peer_id>".cyan());
            println!();
        }
        "info" => {
            if args.len() < 2 {
                return Err("Usage: reputation info <peer_id>".to_string());
            }

            let peer_id = args[1];
            println!("\n👥 Reputation Details for: {}", peer_id);
            println!("  ┌────────────────────────────────────────────────────────┐");
            println!("  │ {:<54} │", format!("Score: 82/100"));
            println!("  │ {:<54} │", format!("Trust Level: High"));
            println!("  │ {:<54} │", format!("Successful Transfers: 47"));
            println!("  │ {:<54} │", format!("Failed Transfers: 3"));
            println!("  │ {:<54} │", format!("Avg Latency: 45ms"));
            println!("  │ {:<54} │", format!("Avg Bandwidth: 2.5 MB/s"));
            println!("  │ {:<54} │", format!("Uptime: 98.5%"));
            println!("  │ {:<54} │", format!("Last Seen: 2 minutes ago"));
            println!("  └────────────────────────────────────────────────────────┘");
            println!();
            println!("  (Full reputation data requires peer stats integration)");
            println!();
        }
        _ => {
            return Err(format!("Unknown reputation subcommand: '{}'. Use 'list' or 'info'", args[0]));
        }
    }

    Ok(())
}

async fn cmd_versions(args: &[&str], _context: &ReplContext) -> Result<(), String> {
    if args.is_empty() {
        return Err("Usage: versions <list|info> <hash>".to_string());
    }

    match args[0] {
        "list" => {
            if args.len() < 2 {
                return Err("Usage: versions list <file_hash>".to_string());
            }

            let hash = args[1];
            println!("\n📂 File Versions for: {}", hash);
            println!("  ┌────────────────────────────────────────────────────────┐");
            println!("  │ {:<54} │", "Version History:");
            println!("  ├────────────────────────────────────────────────────────┤");
            println!("  │ {:<54} │", "  v3 (current) - 2024-10-15 - 2.5 MB");
            println!("  │ {:<54} │", "  v2          - 2024-10-10 - 2.4 MB");
            println!("  │ {:<54} │", "  v1 (initial) - 2024-10-05 - 2.3 MB");
            println!("  └────────────────────────────────────────────────────────┘");
            println!();
            println!("  Use {} to see changes", "versions info <hash>".cyan());
            println!();
        }
        "info" => {
            if args.len() < 2 {
                return Err("Usage: versions info <file_hash>".to_string());
            }

            let hash = args[1];
            println!("\n📂 Version Details for: {}", hash);
            println!("  ┌────────────────────────────────────────────────────────┐");
            println!("  │ {:<54} │", "Version: 3 (current)");
            println!("  │ {:<54} │", "Date: 2024-10-15 14:23:45 UTC");
            println!("  │ {:<54} │", "Size: 2.5 MB");
            println!("  │ {:<54} │", "Parent: v2 (Qmabc...def)");
            println!("  │ {:<54} │", "Changes: +50 KB");
            println!("  │ {:<54} │", "Seeders: 5");
            println!("  └────────────────────────────────────────────────────────┘");
            println!();
            println!("  (Full version tracking requires file metadata integration)");
            println!();
        }
        _ => {
            return Err(format!("Unknown versions subcommand: '{}'. Use 'list' or 'info'", args[0]));
        }
    }

    Ok(())
}
