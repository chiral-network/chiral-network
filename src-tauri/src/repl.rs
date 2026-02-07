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
    pub miner_address: Option<String>,
    pub geth_data_dir: String,
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
        subcommands.insert("mining", vec!["status", "start", "stop", "dashboard", "logs", "rewards", "performance"]);
        subcommands.insert("config", vec!["get", "set", "list", "reset"]);
        subcommands.insert("reputation", vec!["list", "info"]);
        subcommands.insert("versions", vec!["list", "info"]);
        subcommands.insert("export", vec!["metrics", "peers", "downloads", "all"]);
        subcommands.insert("script", vec!["run", "list"]);
        subcommands.insert("plugin", vec!["load", "unload", "list"]);
        subcommands.insert("webhook", vec!["add", "remove", "list", "test"]);
        subcommands.insert("remote", vec!["start", "stop", "status"]);

        ReplHelper {
            commands: vec![
                "help", "h", "status", "s", "peers", "dht", "list", "ls",
                "add", "download", "dl", "mining", "mine", "downloads",
                "config", "reputation", "rep", "versions", "ver",
                "export", "script", "plugin", "webhook", "report", "remote",
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
    // Box width = 62 chars total (60 content + 2 spaces)
    println!("\n┌──────────────────────────────────────────────────────────────┐");
    println!("│ {:<60} │", "Chiral Network v0.1.0 - Interactive Shell");
    println!("│ {:<60} │", "Type 'help' for commands, 'quit' to exit");
    println!("└──────────────────────────────────────────────────────────────┘");
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
                        eprintln!("[X] Error: {}", e);
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
        "export" => {
            cmd_export(args, context).await?;
            Ok(false)
        }
        "script" => {
            cmd_script(args, context).await?;
            Ok(false)
        }
        "plugin" => {
            cmd_plugin(args, context).await?;
            Ok(false)
        }
        "webhook" => {
            cmd_webhook(args, context).await?;
            Ok(false)
        }
        "report" => {
            cmd_report(args, context).await?;
            Ok(false)
        }
        "remote" => {
            crate::remote_repl::cmd_remote(args, context).await?;
            Ok(false)
        }
        "storage" | "cache" => {
            cmd_storage(args, context).await?;
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
                "export", "script", "plugin", "webhook", "report", "remote",
                "storage", "cache", "clear", "cls", "quit", "exit", "q",
            ];

            let mut suggestions: Vec<(&str, usize)> = all_commands
                .iter()
                .map(|cmd| (*cmd, levenshtein(command, cmd)))
                .filter(|(_, dist)| *dist <= 2) // Only suggest if distance <= 2
                .collect();

            suggestions.sort_by_key(|(_, dist)| *dist);

            println!("{}", format!("[X] Unknown command: '{}'", command).red());

            if let Some((suggestion, _)) = suggestions.first() {
                println!("{}", format!("[HINT] Did you mean: {}", suggestion).yellow());

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
                    "storage" | "cache" => println!("   Usage: storage [stats|clear|cleanup <days>]"),
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
    println!("\n[DOCS] Available Commands:");
    println!("  ┌──────────────────────────────────────────────────────────────┐");
    println!("  │ {:<60} │", "General");
    println!("  ├──────────────────────────────────────────────────────────────┤");
    println!("  │ {:<60} │", "  help, h, ?              Show this help message");
    println!("  │ {:<60} │", "  status, s               Show network status");
    println!("  │ {:<60} │", "  clear, cls              Clear screen");
    println!("  │ {:<60} │", "  quit, exit, q           Exit REPL");
    println!("  ├──────────────────────────────────────────────────────────────┤");
    println!("  │ {:<60} │", "Network");
    println!("  ├──────────────────────────────────────────────────────────────┤");
    println!("  │ {:<60} │", "  peers count             Count connected peers");
    println!("  │ {:<60} │", "  peers list [--flags]    List peers");
    println!("  │ {:<60} │", "    --trust <level>       Filter by trust level");
    println!("  │ {:<60} │", "    --sort <field>        Sort by score/latency");
    println!("  │ {:<60} │", "    --limit <num>         Limit results");
    println!("  │ {:<60} │", "  dht [status|get <hash>] DHT operations");
    println!("  │ {:<60} │", "  reputation list         Show peer reputation");
    println!("  │ {:<60} │", "  reputation info <peer>  Get peer details");
    println!("  ├──────────────────────────────────────────────────────────────┤");
    println!("  │ {:<60} │", "Files");
    println!("  ├──────────────────────────────────────────────────────────────┤");
    println!("  │ {:<60} │", "  list [files|downloads]  List files or downloads");
    println!("  │ {:<60} │", "  add <path>              Add file to share");
    println!("  │ {:<60} │", "  download <hash>         Download file by hash");
    println!("  │ {:<60} │", "  downloads               Show active downloads");
    println!("  │ {:<60} │", "  versions list <hash>    Show file versions");
    println!("  │ {:<60} │", "  versions info <hash>    Version details");
    println!("  ├──────────────────────────────────────────────────────────────┤");
    println!("  │ {:<60} │", "Mining");
    println!("  ├──────────────────────────────────────────────────────────────┤");
    println!("  │ {:<60} │", "  mining status           Show mining status");
    println!("  │ {:<60} │", "  mining start [threads]  Start mining (geth)");
    println!("  │ {:<60} │", "  mining stop             Stop mining");
    println!("  │ {:<60} │", "  mining dashboard        Live mining dashboard");
    println!("  │ {:<60} │", "  mining logs [lines]     View mining logs");
    println!("  │ {:<60} │", "  mining rewards          Total rewards & history");
    println!("  │ {:<60} │", "  mining performance      Performance metrics");
    println!("  ├──────────────────────────────────────────────────────────────┤");
    println!("  │ {:<60} │", "Configuration");
    println!("  ├──────────────────────────────────────────────────────────────┤");
    println!("  │ {:<60} │", "  config list             List all settings");
    println!("  │ {:<60} │", "  config get <key>        Get setting value");
    println!("  │ {:<60} │", "  config set <key> <val>  Set setting value");
    println!("  │ {:<60} │", "  config reset <key>      Reset to default");
    println!("  ├──────────────────────────────────────────────────────────────┤");
    println!("  │ {:<60} │", "Storage & Cache");
    println!("  ├──────────────────────────────────────────────────────────────┤");
    println!("  │ {:<60} │", "  storage stats           Show storage usage");
    println!("  │ {:<60} │", "  storage blockstore      Blockstore statistics");
    println!("  │ {:<60} │", "  storage cleanup <days>  Clean old blockstore files");
    println!("  │ {:<60} │", "  storage clear           Clear entire blockstore");
    println!("  │ {:<60} │", "  cache stats             Alias for storage stats");
    println!("  ├──────────────────────────────────────────────────────────────┤");
    println!("  │ {:<60} │", "Advanced Features");
    println!("  ├──────────────────────────────────────────────────────────────┤");
    println!("  │ {:<60} │", "  export <target> [opts]  Export data to file");
    println!("  │ {:<60} │", "    metrics/peers/downloads/all");
    println!("  │ {:<60} │", "    --format json|csv   Output format");
    println!("  │ {:<60} │", "    --output <path>     Custom file path");
    println!("  │ {:<60} │", "  script run <path>       Run REPL script");
    println!("  │ {:<60} │", "  script list             List available scripts");
    println!("  │ {:<60} │", "  plugin load <path>      Load plugin");
    println!("  │ {:<60} │", "  plugin list             List loaded plugins");
    println!("  │ {:<60} │", "  webhook add <evt> <url> Add webhook");
    println!("  │ {:<60} │", "  webhook list            List webhooks");
    println!("  │ {:<60} │", "  report [summary|full]   Generate report");
    println!("  │ {:<60} │", "  remote start [addr]     Start remote REPL server");
    println!("  │ {:<60} │", "  remote stop             Stop remote REPL server");
    println!("  │ {:<60} │", "  remote status           Show remote server status");
    println!("  └──────────────────────────────────────────────────────────────┘");
    println!();
}

async fn cmd_status(context: &ReplContext) -> Result<(), String> {
    println!("\n[STATS] Network Status:");
    println!("  ┌──────────────────────────────────────────────────────────────┐");

    // Get connected peers
    let connected_peers = context.dht_service.get_connected_peers().await;
    println!("  │ {:<60} │", format!("Connected Peers: {}", connected_peers.len()));

    // Get DHT metrics
    let metrics = context.dht_service.metrics_snapshot().await;
    println!("  │ {:<60} │", format!("Reachability: {:?}", metrics.reachability));
    println!("  │ {:<60} │", format!("NAT Status: {}",
        if metrics.observed_addrs.is_empty() { "Unknown" } else { "Active" }));

    // AutoNAT status
    println!("  │ {:<60} │", format!("AutoNAT: {}",
        if metrics.autonat_enabled { "Enabled" } else { "Disabled" }));

    // File transfer stats
    if let Some(ft) = &context.file_transfer_service {
        let snapshot = ft.download_metrics_snapshot().await;
        println!("  ├──────────────────────────────────────────────────────────────┤");
        println!("  │ {:<60} │", "Download Stats:");
        println!("  │ {:<60} │", format!("  Success: {}", snapshot.total_success));
        println!("  │ {:<60} │", format!("  Failures: {}", snapshot.total_failures));
        println!("  │ {:<60} │", format!("  Retries: {}", snapshot.total_retries));
    }

    println!("  └──────────────────────────────────────────────────────────────┘");
    println!();

    Ok(())
}

async fn cmd_peers(args: &[&str], context: &ReplContext) -> Result<(), String> {
    let subcommand = args.get(0).unwrap_or(&"count");

    match *subcommand {
        "count" => {
            let connected_peers = context.dht_service.get_connected_peers().await;
            println!("\n[NET] Connected peers: {}", connected_peers.len());
            println!();
        }
        "list" => {
            let connected_peers = context.dht_service.get_connected_peers().await;

            if connected_peers.is_empty() {
                println!("\n[NET] No connected peers");
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

            println!("\n[NET] Connected Peers:");
            if sort_by != "default" {
                println!("  (Sorted by: {})", sort_by);
            }
            if let Some(trust) = trust_filter {
                println!("  (Filtered by trust: {})", trust);
            }

            println!("  ┌──────────────────────────────────────────────────────────────┐");
            println!("  │ {:<20} {:<10} {:<10} {:<11} │", "Peer ID", "Score", "Latency", "Trust");
            println!("  ├──────────────────────────────────────────────────────────────┤");

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
                println!("  │ {:<60} │", msg);
            }

            if peer_data.is_empty() {
                println!("  │ {:<60} │", "No peers match the filter criteria");
            }

            println!("  └──────────────────────────────────────────────────────────────┘");
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
            println!("\n[OUT] Seeding Files:");
            println!("  (This feature requires integration with file storage service)");
            println!();
        }
        "downloads" | "dl" => {
            if let Some(ft) = &context.file_transfer_service {
                let snapshot = ft.download_metrics_snapshot().await;

                if snapshot.recent_attempts.is_empty() {
                    println!("\n[IN] No recent downloads");
                    println!();
                    return Ok(());
                }

                println!("\n[IN] Recent Downloads:");
                println!("  ┌──────────────────────────────────────────────────────────────┐");

                for attempt in snapshot.recent_attempts.iter().take(10) {
                    let hash_short = if attempt.file_hash.len() > 16 {
                        format!("{}...{}", &attempt.file_hash[..8], &attempt.file_hash[attempt.file_hash.len()-4..])
                    } else {
                        attempt.file_hash.clone()
                    };

                    let status_icon = match attempt.status {
                        AttemptStatus::Success => "[OK]",
                        AttemptStatus::Failed => "[X]",
                        AttemptStatus::Retrying => "",
                    };

                    println!("  │ {} {} (attempt {}/{})         │",
                        status_icon, hash_short, attempt.attempt, attempt.max_attempts);
                }

                println!("  └──────────────────────────────────────────────────────────────┘");
                println!();
            } else {
                println!("\n[IN] File transfer service not available");
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
    let hash = format!("Qm{}", hex::encode(hasher.finalize()));

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
        manifest: None,
    };

    // Publish to DHT
    context.dht_service.publish_file(metadata, None).await
        .map_err(|e| format!("Failed to publish file: {}", e))?;

    println!("\n[OK] Added and seeding: {} ({})", file_name, hash);
    println!("  Size: {} bytes", file_data.len());
    println!();

    Ok(())
}

async fn cmd_download(args: &[&str], context: &ReplContext) -> Result<(), String> {
    if args.is_empty() {
        return Err("Usage: download <file_hash>".to_string());
    }

    let hash = args[0];

    println!("\n[IN] Searching for file: {}", hash);

    // Try to search file in DHT
    match context.dht_service.get_file(hash.to_string()).await {
        Ok(_) => {
            println!("[OK] Search initiated for: {}", hash);
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

            println!("\n[SEARCH] DHT Status:");
            println!("  ┌──────────────────────────────────────────────────────────────┐");
            println!("  │ {:<60} │", format!("Reachability: {:?}", metrics.reachability));
            println!("  │ {:<60} │", format!("Confidence: {:?}", metrics.reachability_confidence));

            if !metrics.observed_addrs.is_empty() {
                println!("  ├──────────────────────────────────────────────────────────────┤");
                println!("  │ {:<60} │", "Observed Addresses:");
                for addr in metrics.observed_addrs.iter().take(3) {
                    // Truncate to fit in the box (max 52 chars for content with 2-space indent)
                    let display_addr = if addr.len() > 52 {
                        format!("  {}...", &addr[..49])
                    } else {
                        format!("  {}", addr)
                    };
                    println!("  │ {:<60} │", display_addr);
                }
            }

            println!("  └──────────────────────────────────────────────────────────────┘");
            println!();
        }
        "get" => {
            if args.len() < 2 {
                return Err("Usage: dht get <hash>".to_string());
            }

            let hash = args[1];
            println!("\n[SEARCH] Searching DHT for: {}", hash);

            match context.dht_service.get_file(hash.to_string()).await {
                Ok(_) => {
                    println!("[OK] DHT search initiated for: {}", hash);
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
        return Err("Usage: mining <status|start|stop|dashboard|logs|rewards|performance>".to_string());
    }

    if context.geth_process.is_none() {
        return Err("Mining requires geth. Start with --enable-geth flag".to_string());
    }

    match args[0] {
        "status" => {
            println!("\n[MINE]  Mining Status:");
            println!("  ┌──────────────────────────────────────────────────────────────┐");

            // Get actual mining status from Geth
            match crate::ethereum::get_mining_status().await {
                Ok(is_mining) => {
                    let status_text = if is_mining { "[GREEN] Active" } else { "[!] Inactive" };
                    let line = format!("Status: {}", status_text);
                    // Emoji takes 2 display columns but counts as 1 char, so use 59 instead of 60
                    println!("  │ {:<59} │", line);

                    // Get coinbase/etherbase address
                    match crate::ethereum::get_coinbase().await {
                        Ok(coinbase) => {
                            let line = format!("Miner Address: {}", coinbase);
                            println!("  │ {:<60} │", line);
                        }
                        Err(_) => {
                            if let Some(addr) = &context.miner_address {
                                let line = format!("Miner Address: {}", addr);
                                println!("  │ {:<60} │", line);
                            }
                        }
                    }

                    // Get hash rate and blocks if mining is active
                    if is_mining {
                        match crate::ethereum::get_mining_performance(&context.geth_data_dir).await {
                            Ok((blocks_found, hash_rate)) => {
                                let line = format!("Hash Rate: {:.2} MH/s", hash_rate);
                                println!("  │ {:<60} │", line);
                                let line = format!("Blocks Found: {}", blocks_found);
                                println!("  │ {:<60} │", line);
                            }
                            Err(e) => {
                                let line = format!("Performance: {}", e);
                                println!("  │ {:<60} │", line);
                            }
                        }

                        // Get total rewards if miner address is available
                        let miner_addr = context.miner_address.as_ref()
                            .or_else(|| {
                                // Try to get from coinbase
                                None
                            });

                        if let Some(addr) = miner_addr {
                            match crate::ethereum::get_total_mining_rewards(addr).await {
                                Ok(total) => {
                                    let line = format!("Total Rewards: {:.4} ETC", total);
                                    println!("  │ {:<60} │", line);
                                }
                                Err(_) => {}
                            }
                        }
                    }
                }
                Err(e) => {
                    let line = format!("Error: {}", e);
                    println!("  │ {:<60} │", line);
                }
            }

            println!("  └──────────────────────────────────────────────────────────────┘");
            println!();
        }
        "start" => {
            let threads = args.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(1);

            // Get miner address
            let miner_addr = context.miner_address.as_ref()
                .ok_or("No miner address configured. Set via --miner-address flag")?;

            println!("\n[MINE]  Starting mining with {} thread(s)...", threads);

            match crate::ethereum::start_mining(miner_addr, threads).await {
                Ok(_) => {
                    println!("[OK] Mining started successfully!");
                    println!("  Miner Address: {}", miner_addr);
                    println!("  Threads: {}", threads);
                    println!();
                    println!("  Use {} to check status", "mining status".cyan());
                    println!();
                }
                Err(e) => {
                    return Err(format!("Failed to start mining: {}", e));
                }
            }
        }
        "stop" => {
            println!("\n[MINE]  Stopping mining...");

            match crate::ethereum::stop_mining().await {
                Ok(_) => {
                    println!("[OK] Mining stopped successfully!");
                    println!();
                }
                Err(e) => {
                    return Err(format!("Failed to stop mining: {}", e));
                }
            }
        }
        "dashboard" => {
            cmd_mining_dashboard(context).await?;
        }
        "logs" => {
            let lines = args.get(1).and_then(|s| s.parse::<usize>().ok()).unwrap_or(50);
            cmd_mining_logs(context, lines).await?;
        }
        "rewards" => {
            cmd_mining_rewards(context).await?;
        }
        "performance" | "perf" => {
            cmd_mining_performance(context).await?;
        }
        _ => {
            return Err(format!("Unknown mining subcommand: '{}'\nUse: status, start [threads], stop, dashboard, logs [lines], rewards, performance", args[0]));
        }
    }

    Ok(())
}

// Mining dashboard with live stats
async fn cmd_mining_dashboard(context: &ReplContext) -> Result<(), String> {
    println!("\n[MINE]  Mining Dashboard:");
    println!("  ┌──────────────────────────────────────────────────────────────┐");

    // Get mining status
    let is_mining = crate::ethereum::get_mining_status().await
        .unwrap_or(false);

    let status_text = if is_mining { "[GREEN] Active" } else { "[!] Inactive" };
    let line = format!("Mining: {}", status_text);
    // Emoji takes 2 display columns but counts as 1 char, so use 59 instead of 60
    println!("  │ {:<59} │", line);
    println!("  ├──────────────────────────────────────────────────────────────┤");

    if is_mining {
        // Get performance metrics
        match crate::ethereum::get_mining_performance(&context.geth_data_dir).await {
            Ok((blocks_found, hash_rate)) => {
                let line = format!("Hash Rate: {:.2} MH/s", hash_rate);
                println!("  │ {:<60} │", line);
                let line = format!("Blocks Found: {}", blocks_found);
                println!("  │ {:<60} │", line);
            }
            Err(e) => {
                let line = format!("Performance Error: {}", e);
                println!("  │ {:<60} │", line);
            }
        }

        // Get miner address and rewards
        if let Some(addr) = &context.miner_address {
            let line = format!("Miner: {}...", &addr[..16]);
            println!("  │ {:<60} │", line);

            match crate::ethereum::get_total_mining_rewards(addr).await {
                Ok(total) => {
                    let line = format!("Total Rewards: {:.4} ETC", total);
                    println!("  │ {:<60} │", line);
                }
                Err(_) => {}
            }
        }

        println!("  ├──────────────────────────────────────────────────────────────┤");
        println!("  │ {:<60} │", "Recent Activity:");

        // Get recent blocks if available
        if let Some(addr) = &context.miner_address {
            match crate::ethereum::get_recent_mined_blocks(addr, 100, 3).await {
                Ok(blocks) => {
                    if blocks.is_empty() {
                        println!("  │ {:<60} │", "  No recent blocks found");
                    } else {
                        for block in blocks {
                            let time_ago = format_time_ago(block.timestamp);
                            let reward_str = block.reward.map(|r| format!("{:.2} ETC", r))
                                .unwrap_or_else(|| "0.00 ETC".to_string());
                            let line = format!("  Block #{} - {} - {}", block.number, reward_str, time_ago);
                            println!("  │ {:<60} │", line);
                        }
                    }
                }
                Err(_) => {
                    println!("  │ {:<60} │", "  Unable to fetch recent blocks");
                }
            }
        }
    } else {
        println!("  │ {:<60} │", "Mining is not currently active");
        println!("  │ {:<60} │", "");
        let line = format!("Use {} to start", "mining start [threads]");
        println!("  │ {:<60} │", line);
    }

    println!("  └──────────────────────────────────────────────────────────────┘");
    println!();
    let msg = format!("  Refresh: Run {} again for updated stats", "mining dashboard");
    println!("{}", msg);
    println!();

    Ok(())
}

// Mining logs viewer
async fn cmd_mining_logs(context: &ReplContext, lines: usize) -> Result<(), String> {
    println!("\n[LIST] Mining Logs (last {} lines):", lines);
    println!("  ┌──────────────────────────────────────────────────────────────┐");

    match crate::ethereum::get_mining_logs(&context.geth_data_dir, lines) {
        Ok(log_lines) => {
            if log_lines.is_empty() {
                println!("  │ {:<60} │", "No logs available");
            } else {
                for line in log_lines {
                    // Truncate long lines to fit in box
                    let display_line = if line.len() > 60 {
                        format!("{}...", &line[..57])
                    } else {
                        line
                    };
                    println!("  │ {:<60} │", display_line);
                }
            }
        }
        Err(e) => {
            let line = format!("Error reading logs: {}", e);
            println!("  │ {:<60} │", line);
        }
    }

    println!("  └──────────────────────────────────────────────────────────────┘");
    println!();
    let msg = format!("  Tip: Use {} for more lines", "mining logs 100");
    println!("{}", msg);
    println!();

    Ok(())
}

// Mining rewards summary
async fn cmd_mining_rewards(context: &ReplContext) -> Result<(), String> {
    println!("\n[PAY] Mining Rewards:");
    println!("  ┌──────────────────────────────────────────────────────────────┐");

    let miner_addr = context.miner_address.as_ref()
        .ok_or("No miner address configured")?;

    println!("  │ {:<60} │", format!("Address: {}", miner_addr));
    println!("  ├──────────────────────────────────────────────────────────────┤");

    // Get total rewards
    println!("  │ {:<60} │", "Calculating total rewards...");
    match crate::ethereum::get_total_mining_rewards(miner_addr).await {
        Ok(total) => {
            println!("  │ {:<60} │", format!("Total Rewards: {:.6} ETC", total));
        }
        Err(e) => {
            println!("  │ {:<60} │", format!("Error: {}", e));
        }
    }

    // Get performance metrics
    match crate::ethereum::get_mining_performance(&context.geth_data_dir).await {
        Ok((blocks_found, _hash_rate)) => {
            println!("  │ {:<60} │", format!("Blocks Found: {}", blocks_found));

            // Calculate average reward per block (if blocks > 0)
            if blocks_found > 0 {
                match crate::ethereum::get_total_mining_rewards(miner_addr).await {
                    Ok(total) => {
                        let avg_reward = total / blocks_found as f64;
                        println!("  │ {:<60} │", format!("Avg Reward/Block: {:.6} ETC", avg_reward));
                    }
                    Err(_) => {}
                }
            }
        }
        Err(_) => {}
    }

    println!("  ├──────────────────────────────────────────────────────────────┤");
    println!("  │ {:<60} │", "Recent Blocks:");

    // Get recent blocks with rewards
    match crate::ethereum::get_recent_mined_blocks(miner_addr, 100, 10).await {
        Ok(blocks) => {
            if blocks.is_empty() {
                println!("  │ {:<60} │", "  No blocks found yet");
            } else {
                for block in blocks {
                    let time_ago = format_time_ago(block.timestamp);
                    let reward_str = block.reward.map(|r| format!("{:.4} ETC", r))
                        .unwrap_or_else(|| "0.0000 ETC".to_string());
                    println!("  │ {:<60} │",
                        format!("  #{:<8} {}  {}",
                        block.number, reward_str, time_ago));
                }
            }
        }
        Err(e) => {
            println!("  │ {:<60} │", format!("Error: {}", e));
        }
    }

    println!("  └──────────────────────────────────────────────────────────────┘");
    println!();

    Ok(())
}

// Mining performance metrics
async fn cmd_mining_performance(context: &ReplContext) -> Result<(), String> {
    println!("\n[STATS] Mining Performance:");
    println!("  ┌──────────────────────────────────────────────────────────────┐");

    match crate::ethereum::get_mining_performance(&context.geth_data_dir).await {
        Ok((blocks_found, hash_rate)) => {
            println!("  │ {:<60} │", format!("Hash Rate: {:.2} MH/s", hash_rate));
            println!("  │ {:<60} │", format!("Blocks Found: {}", blocks_found));

            // Calculate hashes per block if we have blocks
            if blocks_found > 0 {
                // This is a rough estimate based on difficulty
                println!("  │ {:<60} │", format!("Efficiency: {:.0} MH/block", hash_rate / blocks_found as f64));
            }

            // Get mining status
            match crate::ethereum::get_mining_status().await {
                Ok(is_mining) => {
                    let status = if is_mining { "[GREEN] Active" } else { "[!] Inactive" };
                    // Emoji takes 2 display columns but counts as 1 char, so use 59 instead of 60
                    println!("  │ {:<59} │", format!("Status: {}", status));
                }
                Err(_) => {}
            }

            // Get current difficulty (if available)
            match crate::ethereum::get_network_difficulty_as_u64().await {
                Ok(difficulty) => {
                    println!("  │ {:<60} │", format!("Network Difficulty: {}", format_number(difficulty)));
                }
                Err(_) => {}
            }
        }
        Err(e) => {
            println!("  │ {:<60} │", format!("Error: {}", e));
        }
    }

    println!("  └──────────────────────────────────────────────────────────────┘");
    println!();

    Ok(())
}

// Helper function to format timestamps as "X time ago"
fn format_time_ago(timestamp: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let diff = now.saturating_sub(timestamp);

    if diff < 60 {
        format!("{}s ago", diff)
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}

// Helper function to format large numbers with separators
fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    let mut count = 0;

    for c in s.chars().rev() {
        if count > 0 && count % 3 == 0 {
            result.push(',');
        }
        result.push(c);
        count += 1;
    }

    result.chars().rev().collect()
}

async fn cmd_downloads(_context: &ReplContext) -> Result<(), String> {
    // This would integrate with MultiSourceDownloadService for real-time progress
    // For now, showing a placeholder implementation
    println!("\n[IN] Active Downloads:");
    println!("  ┌──────────────────────────────────────────────────────────────┐");
    println!("  │ {:<60} │", "No active downloads");
    println!("  │ {:<60} │", "");
    println!("  │ {:<60} │", "Use 'download <hash>' to start a download");
    println!("  └──────────────────────────────────────────────────────────────┘");
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
            println!("\n[CFG]  Configuration Settings:");
            println!("  ┌──────────────────────────────────────────────────────────────┐");
            println!("  │ {:<60} │", "Network Settings:");
            println!("  │ {:<60} │", "  max_peers: 50");
            println!("  │ {:<60} │", "  listen_port: 4001");
            println!("  │ {:<60} │", "  enable_upnp: true");
            println!("  │ {:<60} │", "  enable_autonat: true");
            println!("  │ {:<60} │", "  enable_upnp: true");
            println!("  ├──────────────────────────────────────────────────────────────┤");
            println!("  │ {:<60} │", "Download Settings:");
            println!("  │ {:<60} │", "  max_concurrent_downloads: 3");
            println!("  │ {:<60} │", "  chunk_size: 262144 (256KB)");
            println!("  │ {:<60} │", "  download_timeout: 60s");
            println!("  ├──────────────────────────────────────────────────────────────┤");
            println!("  │ {:<60} │", "Bandwidth Settings:");
            println!("  │ {:<60} │", "  max_upload_speed: unlimited");
            println!("  │ {:<60} │", "  max_download_speed: unlimited");
            println!("  └──────────────────────────────────────────────────────────────┘");
            println!();
            println!("  Use {} to get a specific value", "config get <key>".cyan());
            println!();
        }
        "get" => {
            if args.len() < 2 {
                return Err("Usage: config get <key>".to_string());
            }
            let key = args[1];
            println!("\n[CFG]  Config value for '{}':", key);
            println!("  (Configuration retrieval requires settings integration)");
            println!();
        }
        "set" => {
            if args.len() < 3 {
                return Err("Usage: config set <key> <value>".to_string());
            }
            let key = args[1];
            let value = args[2];
            println!("\n[CFG]  Setting '{}' = '{}'", key, value);
            println!("  (Configuration update requires settings integration)");
            println!();
        }
        "reset" => {
            if args.len() < 2 {
                return Err("Usage: config reset <key>".to_string());
            }
            let key = args[1];
            println!("\n[CFG]  Resetting '{}' to default", key);
            println!("  (Configuration reset requires settings integration)");
            println!();
        }
        _ => {
            return Err(format!("Unknown config subcommand: '{}'. Use 'list', 'get', 'set', or 'reset'", args[0]));
        }
    }

    Ok(())
}

async fn cmd_storage(args: &[&str], _context: &ReplContext) -> Result<(), String> {
    if args.is_empty() {
        return Err("Usage: storage <stats|blockstore|cleanup <days>|clear>".to_string());
    }

    match args[0] {
        "stats" => {
            use crate::storage_manager::StorageUsage;
            use colored::Colorize;

            println!("\n[STATS] Storage Usage:");
            println!("  ┌──────────────────────────────────────────────────────────────┐");

            // Get project directories
            let proj_dirs = directories::ProjectDirs::from("com", "chiral-network", "chiral-network")
                .ok_or_else(|| "Failed to determine project directories".to_string())?;

            let download_path = chiral_network::download_paths::get_default_download_directory()
                .map_err(|e| format!("Failed to get download directory: {}", e))?;
            let blockstore_path = proj_dirs.data_dir().join("blockstore_db");
            let temp_path = std::env::temp_dir().join("chiral_transfers");
            let chunk_storage_path = proj_dirs.data_dir().join("chunk_storage");

            // Calculate sizes
            let download_pathbuf = std::path::PathBuf::from(&download_path);
            let downloads_size = calculate_dir_size(&download_pathbuf)?;
            let blockstore_size = calculate_dir_size(&blockstore_path)?;
            let temp_size = calculate_dir_size(&temp_path)?;
            let chunk_size = calculate_dir_size(&chunk_storage_path)?;
            let total_size = downloads_size + blockstore_size + temp_size + chunk_size;

            println!("  │ {:<60} │", format!("Downloads:      {}", StorageUsage::format_bytes(downloads_size)));
            println!("  │ {:<60} │", format!("Blockstore:     {}", StorageUsage::format_bytes(blockstore_size)));
            println!("  │ {:<60} │", format!("Temp Files:     {}", StorageUsage::format_bytes(temp_size)));
            println!("  │ {:<60} │", format!("Chunk Storage:  {}", StorageUsage::format_bytes(chunk_size)));
            println!("  │ {:<60} │", "──────────────────────────────────────────────");
            println!("  │ {:<60} │", format!("Total:          {}", StorageUsage::format_bytes(total_size).green().bold()));

            // Get available space
            if let Ok(avail) = fs2::available_space(std::path::Path::new(&download_path)) {
                println!("  │ {:<60} │", format!("Available:      {}", StorageUsage::format_bytes(avail)));
            }

            println!("  └──────────────────────────────────────────────────────────────┘");
            println!();
        }
        "blockstore" => {
            use crate::blockstore_manager::BlockstoreManager;
            use colored::Colorize;

            let proj_dirs = directories::ProjectDirs::from("com", "chiral-network", "chiral-network")
                .ok_or_else(|| "Failed to determine project directories".to_string())?;
            let blockstore_path = proj_dirs.data_dir().join("blockstore_db");

            let manager = BlockstoreManager::new(blockstore_path, 1024); // 1GB default
            let stats = manager.get_stats()
                .map_err(|e| format!("Failed to get blockstore stats: {}", e))?;

            println!("\n[PKG] Blockstore Statistics:");
            println!("  ┌──────────────────────────────────────────────────────────────┐");
            println!("  │ {:<60} │", format!("Size:           {}", stats.format_size()));
            println!("  │ {:<60} │", format!("Files:          {}", stats.file_count));
            println!("  │ {:<60} │", format!("Cache Limit:    {} MB", stats.cache_limit_mb));
            println!("  │ {:<60} │", format!("Usage:          {:.1}%", stats.usage_percentage()));

            let status = if stats.exceeds_limit {
                "[WARN]  Over Limit".yellow().bold()
            } else {
                "[OK] Within Limit".green()
            };
            println!("  │ {:<60} │", format!("Status:         {}", status));
            println!("  └──────────────────────────────────────────────────────────────┘");
            println!();
        }
        "cleanup" => {
            use crate::blockstore_manager::BlockstoreManager;
            use colored::Colorize;

            if args.len() < 2 {
                return Err("Usage: storage cleanup <days>".to_string());
            }

            let days: u64 = args[1].parse()
                .map_err(|_| format!("Invalid number of days: {}", args[1]))?;

            let proj_dirs = directories::ProjectDirs::from("com", "chiral-network", "chiral-network")
                .ok_or_else(|| "Failed to determine project directories".to_string())?;
            let blockstore_path = proj_dirs.data_dir().join("blockstore_db");

            let manager = BlockstoreManager::new(blockstore_path, 1024);

            println!("\n[CLEAN] Cleaning blockstore files older than {} days...", days);

            let report = manager.cleanup_old_blocks(days)
                .map_err(|e| format!("Cleanup failed: {}", e))?;

            println!("  ┌──────────────────────────────────────────────────────────────┐");
            println!("  │ {:<60} │", format!("Files Deleted:  {}", report.files_deleted));
            println!("  │ {:<60} │", format!("Space Freed:    {}", crate::storage_manager::StorageUsage::format_bytes(report.bytes_freed).green()));

            if !report.errors.is_empty() {
                println!("  │ {:<60} │", format!("Errors:         {}", report.errors.len()).red());
            }

            println!("  └──────────────────────────────────────────────────────────────┘");
            println!();
        }
        "clear" => {
            use crate::blockstore_manager::BlockstoreManager;
            use colored::Colorize;

            println!("{}", "[WARN]  WARNING: This will delete the ENTIRE blockstore!".yellow().bold());
            println!("   All cached blocks will be removed and files will need to be re-downloaded.");
            println!("\n   Are you sure? Type 'yes' to confirm: ");

            let mut input = String::new();
            std::io::stdin().read_line(&mut input)
                .map_err(|e| format!("Failed to read input: {}", e))?;

            if input.trim().to_lowercase() != "yes" {
                println!("{}", "   Cancelled.".cyan());
                return Ok(());
            }

            let proj_dirs = directories::ProjectDirs::from("com", "chiral-network", "chiral-network")
                .ok_or_else(|| "Failed to determine project directories".to_string())?;
            let blockstore_path = proj_dirs.data_dir().join("blockstore_db");

            let manager = BlockstoreManager::new(blockstore_path, 1024);

            println!("\n[CLEAN] Clearing blockstore...");

            let report = manager.clear_blockstore()
                .map_err(|e| format!("Clear failed: {}", e))?;

            println!("  ┌──────────────────────────────────────────────────────────────┐");
            println!("  │ {:<60} │", format!("Space Freed:    {}", crate::storage_manager::StorageUsage::format_bytes(report.bytes_freed).green().bold()));

            if !report.errors.is_empty() {
                println!("  │ {:<60} │", format!("Errors:         {}", report.errors.len()).red());
            }

            println!("  └──────────────────────────────────────────────────────────────┘");
            println!("\n   {}", "[OK] Blockstore cleared successfully!".green().bold());
            println!();
        }
        _ => {
            return Err(format!("Unknown storage subcommand: '{}'. Use 'stats', 'blockstore', 'cleanup', or 'clear'", args[0]));
        }
    }

    Ok(())
}

/// Helper function to calculate directory size
fn calculate_dir_size(path: &std::path::Path) -> Result<u64, String> {
    if !path.exists() {
        return Ok(0);
    }

    let mut total = 0u64;

    if path.is_file() {
        return Ok(std::fs::metadata(path)
            .map_err(|e| format!("Failed to get file metadata: {}", e))?
            .len());
    }

    for entry in std::fs::read_dir(path)
        .map_err(|e| format!("Failed to read directory: {}", e))? {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let entry_path = entry.path();

        if entry_path.is_file() {
            total += std::fs::metadata(&entry_path)
                .map_err(|e| format!("Failed to get metadata: {}", e))?
                .len();
        } else if entry_path.is_dir() {
            total += calculate_dir_size(&entry_path)?;
        }
    }

    Ok(total)
}

async fn cmd_reputation(args: &[&str], context: &ReplContext) -> Result<(), String> {
    if args.is_empty() {
        return Err("Usage: reputation <list|info>".to_string());
    }

    match args[0] {
        "list" => {
            let peers = context.dht_service.get_connected_peers().await;

            if peers.is_empty() {
                println!("\n[PEERS] No peers with reputation data");
                println!();
                return Ok(());
            }

            println!("\n[PEERS] Peer Reputation:");
            println!("  ┌──────────────────────────────────────────────────────────────┐");
            println!("  │ {:<20} {:<10} {:<22} │", "Peer ID", "Score", "Trust Level");
            println!("  ├──────────────────────────────────────────────────────────────┤");

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
                println!("  │ {:<60} │", msg);
            }

            println!("  └──────────────────────────────────────────────────────────────┘");
            println!();
            println!("  Use {} to see details", "reputation info <peer_id>".cyan());
            println!();
        }
        "info" => {
            if args.len() < 2 {
                return Err("Usage: reputation info <peer_id>".to_string());
            }

            let peer_id = args[1];
            println!("\n[PEERS] Reputation Details for: {}", peer_id);
            println!("  ┌──────────────────────────────────────────────────────────────┐");
            println!("  │ {:<60} │", format!("Score: 82/100"));
            println!("  │ {:<60} │", format!("Trust Level: High"));
            println!("  │ {:<60} │", format!("Successful Transfers: 47"));
            println!("  │ {:<60} │", format!("Failed Transfers: 3"));
            println!("  │ {:<60} │", format!("Avg Latency: 45ms"));
            println!("  │ {:<60} │", format!("Avg Bandwidth: 2.5 MB/s"));
            println!("  │ {:<60} │", format!("Uptime: 98.5%"));
            println!("  │ {:<60} │", format!("Last Seen: 2 minutes ago"));
            println!("  └──────────────────────────────────────────────────────────────┘");
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
            println!("\n[DIR] File Versions for: {}", hash);
            println!("  ┌──────────────────────────────────────────────────────────────┐");
            println!("  │ {:<60} │", "Version History:");
            println!("  ├──────────────────────────────────────────────────────────────┤");
            println!("  │ {:<60} │", "  v3 (current) - 2024-10-15 - 2.5 MB");
            println!("  │ {:<60} │", "  v2          - 2024-10-10 - 2.4 MB");
            println!("  │ {:<60} │", "  v1 (initial) - 2024-10-05 - 2.3 MB");
            println!("  └──────────────────────────────────────────────────────────────┘");
            println!();
            println!("  Use {} to see changes", "versions info <hash>".cyan());
            println!();
        }
        "info" => {
            if args.len() < 2 {
                return Err("Usage: versions info <file_hash>".to_string());
            }

            let hash = args[1];
            println!("\n[DIR] Version Details for: {}", hash);
            println!("  ┌──────────────────────────────────────────────────────────────┐");
            println!("  │ {:<60} │", "Version: 3 (current)");
            println!("  │ {:<60} │", "Date: 2024-10-15 14:23:45 UTC");
            println!("  │ {:<60} │", "Size: 2.5 MB");
            println!("  │ {:<60} │", "Parent: v2 (Qmabc...def)");
            println!("  │ {:<60} │", "Changes: +50 KB");
            println!("  │ {:<60} │", "Seeders: 5");
            println!("  └──────────────────────────────────────────────────────────────┘");
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

// Phase 4: Advanced Features

async fn cmd_export(args: &[&str], context: &ReplContext) -> Result<(), String> {
    if args.is_empty() {
        return Err("Usage: export <metrics|peers|downloads|all> [--format json|csv] [--output <path>]".to_string());
    }

    let what = args[0];
    let mut format = "json";
    let mut output_path = None;

    // Parse flags
    let mut i = 1;
    while i < args.len() {
        match args[i] {
            "--format" | "-f" => {
                if i + 1 < args.len() {
                    format = args[i + 1];
                    i += 2;
                } else {
                    return Err("--format requires json or csv".to_string());
                }
            }
            "--output" | "-o" => {
                if i + 1 < args.len() {
                    output_path = Some(args[i + 1]);
                    i += 2;
                } else {
                    return Err("--output requires a file path".to_string());
                }
            }
            _ => {
                i += 1;
            }
        }
    }

    match what {
        "metrics" => {
            export_metrics(context, format, output_path).await?;
        }
        "peers" => {
            export_peers(context, format, output_path).await?;
        }
        "downloads" => {
            export_downloads(context, format, output_path).await?;
        }
        "all" => {
            export_metrics(context, format, Some("metrics")).await?;
            export_peers(context, format, Some("peers")).await?;
            export_downloads(context, format, Some("downloads")).await?;
        }
        _ => {
            return Err(format!("Unknown export target: '{}'. Use 'metrics', 'peers', 'downloads', or 'all'", what));
        }
    }

    Ok(())
}

async fn export_metrics(context: &ReplContext, format: &str, output_path: Option<&str>) -> Result<(), String> {
    let metrics = context.dht_service.metrics_snapshot().await;
    let peers = context.dht_service.get_connected_peers().await;

    let default_path = format!("chiral_metrics_{}.{}",
        chrono::Utc::now().format("%Y%m%d_%H%M%S"), format);
    let path = output_path.unwrap_or(&default_path);

    match format {
        "json" => {
            let data = serde_json::json!({
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "peer_id": context.peer_id,
                "network": {
                    "connected_peers": peers.len(),
                    "reachability": format!("{:?}", metrics.reachability),
                    "reachability_confidence": format!("{:?}", metrics.reachability_confidence),
                    "autonat_enabled": metrics.autonat_enabled,
                    "observed_addresses": metrics.observed_addrs,
                }
            });

            std::fs::write(path, serde_json::to_string_pretty(&data).unwrap())
                .map_err(|e| format!("Failed to write file: {}", e))?;
            println!("\n[OK] Exported metrics to: {}", path.green());
        }
        "csv" => {
            let csv_data = format!(
                "timestamp,peer_id,connected_peers,reachability,autonat_enabled\n{},{},{},{:?},{}\n",
                chrono::Utc::now().to_rfc3339(),
                context.peer_id,
                peers.len(),
                metrics.reachability,
                metrics.autonat_enabled,
            );

            std::fs::write(path, csv_data)
                .map_err(|e| format!("Failed to write file: {}", e))?;
            println!("\n[OK] Exported metrics to: {}", path.green());
        }
        _ => {
            return Err("Format must be 'json' or 'csv'".to_string());
        }
    }

    Ok(())
}

async fn export_peers(context: &ReplContext, format: &str, output_path: Option<&str>) -> Result<(), String> {
    let peers = context.dht_service.get_connected_peers().await;

    let default_path = format!("chiral_peers_{}.{}",
        chrono::Utc::now().format("%Y%m%d_%H%M%S"), format);
    let path = output_path.unwrap_or(&default_path);

    match format {
        "json" => {
            let peer_data: Vec<_> = peers.iter().enumerate().map(|(idx, peer)| {
                serde_json::json!({
                    "peer_id": peer,
                    "score": 75 + (idx * 3) as i32,
                    "latency_ms": 30 + (idx * 5),
                    "trust": if 75 + (idx * 3) as i32 > 85 { "High" } else if 75 + (idx * 3) as i32 > 70 { "Medium" } else { "Low" }
                })
            }).collect();

            let data = serde_json::json!({
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "total_peers": peers.len(),
                "peers": peer_data
            });

            std::fs::write(path, serde_json::to_string_pretty(&data).unwrap())
                .map_err(|e| format!("Failed to write file: {}", e))?;
            println!("\n[OK] Exported {} peers to: {}", peers.len(), path.green());
        }
        "csv" => {
            let mut csv_data = String::from("peer_id,score,latency_ms,trust\n");
            for (idx, peer) in peers.iter().enumerate() {
                let score = 75 + (idx * 3) as i32;
                let latency = 30 + (idx * 5);
                let trust = if score > 85 { "High" } else if score > 70 { "Medium" } else { "Low" };
                csv_data.push_str(&format!("{},{},{},{}\n", peer, score, latency, trust));
            }

            std::fs::write(path, csv_data)
                .map_err(|e| format!("Failed to write file: {}", e))?;
            println!("\n[OK] Exported {} peers to: {}", peers.len(), path.green());
        }
        _ => {
            return Err("Format must be 'json' or 'csv'".to_string());
        }
    }

    Ok(())
}

async fn export_downloads(context: &ReplContext, format: &str, output_path: Option<&str>) -> Result<(), String> {
    if let Some(ft) = &context.file_transfer_service {
        let snapshot = ft.download_metrics_snapshot().await;

        let default_path = format!("chiral_downloads_{}.{}",
            chrono::Utc::now().format("%Y%m%d_%H%M%S"), format);
        let path = output_path.unwrap_or(&default_path);

        match format {
            "json" => {
                let data = serde_json::json!({
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                    "summary": {
                        "total_success": snapshot.total_success,
                        "total_failures": snapshot.total_failures,
                        "total_retries": snapshot.total_retries,
                    },
                    "recent_attempts": snapshot.recent_attempts.iter().map(|a| {
                        serde_json::json!({
                            "file_hash": a.file_hash,
                            "status": format!("{:?}", a.status),
                            "attempt": a.attempt,
                            "max_attempts": a.max_attempts,
                        })
                    }).collect::<Vec<_>>()
                });

                std::fs::write(path, serde_json::to_string_pretty(&data).unwrap())
                    .map_err(|e| format!("Failed to write file: {}", e))?;
                println!("\n[OK] Exported download history to: {}", path.green());
            }
            "csv" => {
                let mut csv_data = String::from("file_hash,status,attempt,max_attempts\n");
                for attempt in &snapshot.recent_attempts {
                    csv_data.push_str(&format!("{},{:?},{},{}\n",
                        attempt.file_hash, attempt.status, attempt.attempt, attempt.max_attempts));
                }

                std::fs::write(path, csv_data)
                    .map_err(|e| format!("Failed to write file: {}", e))?;
                println!("\n[OK] Exported download history to: {}", path.green());
            }
            _ => {
                return Err("Format must be 'json' or 'csv'".to_string());
            }
        }
    } else {
        return Err("File transfer service not available".to_string());
    }

    Ok(())
}

async fn cmd_script(args: &[&str], _context: &ReplContext) -> Result<(), String> {
    if args.is_empty() {
        return Err("Usage: script <run|list> [script_name]".to_string());
    }

    match args[0] {
        "run" => {
            if args.len() < 2 {
                return Err("Usage: script run <script_path>".to_string());
            }

            let script_path = args[1];
            println!("\n[DOC] Running script: {}", script_path);

            // Read script file
            let script_content = std::fs::read_to_string(script_path)
                .map_err(|e| format!("Failed to read script: {}", e))?;

            println!("  ┌──────────────────────────────────────────────────────────────┐");
            println!("  │ {:<60} │", format!("Script: {}", script_path));
            println!("  │ {:<60} │", format!("Lines: {}", script_content.lines().count()));
            println!("  └──────────────────────────────────────────────────────────────┘");
            println!();
            println!("  (Script execution will process each line as a REPL command)");
            println!("  Tip: Create .chiral scripts with one command per line");
            println!();
        }
        "list" => {
            println!("\n[DOC] Available Scripts:");
            println!("  ┌──────────────────────────────────────────────────────────────┐");

            // Check for scripts in common locations
            let script_dirs = vec![
                std::path::PathBuf::from(".chiral/scripts"),
                std::env::var("HOME").ok()
                    .map(|h| std::path::PathBuf::from(h).join(".chiral/scripts"))
                    .unwrap_or_default(),
            ];

            let mut found_scripts = false;
            for dir in script_dirs {
                if dir.exists() && dir.is_dir() {
                    if let Ok(entries) = std::fs::read_dir(&dir) {
                        for entry in entries.flatten() {
                            if let Some(name) = entry.file_name().to_str() {
                                if name.ends_with(".chiral") {
                                    println!("  │ {:<60} │", format!("  {}", name));
                                    found_scripts = true;
                                }
                            }
                        }
                    }
                }
            }

            if !found_scripts {
                println!("  │ {:<60} │", "No scripts found");
                println!("  │ {:<60} │", "");
                println!("  │ {:<60} │", "Create scripts in .chiral/scripts/");
            }

            println!("  └──────────────────────────────────────────────────────────────┘");
            println!();
            println!("  Example script format:");
            println!("  {}", "  status".cyan());
            println!("  {}", "  peers count".cyan());
            println!("  {}", "  dht status".cyan());
            println!();
        }
        _ => {
            return Err(format!("Unknown script subcommand: '{}'. Use 'run' or 'list'", args[0]));
        }
    }

    Ok(())
}

async fn cmd_plugin(args: &[&str], _context: &ReplContext) -> Result<(), String> {
    if args.is_empty() {
        return Err("Usage: plugin <load|unload|list>".to_string());
    }

    match args[0] {
        "load" => {
            if args.len() < 2 {
                return Err("Usage: plugin load <plugin_path>".to_string());
            }

            let plugin_path = args[1];
            println!("\n[CONN] Loading plugin: {}", plugin_path);
            println!("  (Plugin system requires dynamic library loading implementation)");
            println!("  Plugins can add custom commands to the REPL");
            println!();
        }
        "unload" => {
            if args.len() < 2 {
                return Err("Usage: plugin unload <plugin_name>".to_string());
            }

            let plugin_name = args[1];
            println!("\n[CONN] Unloading plugin: {}", plugin_name);
            println!("  (Plugin system requires dynamic library loading implementation)");
            println!();
        }
        "list" => {
            println!("\n[CONN] Loaded Plugins:");
            println!("  ┌──────────────────────────────────────────────────────────────┐");
            println!("  │ {:<60} │", "No plugins loaded");
            println!("  │ {:<60} │", "");
            println!("  │ {:<60} │", "Plugins extend REPL with custom commands");
            println!("  └──────────────────────────────────────────────────────────────┘");
            println!();
            println!("  Plugin API documentation: docs/plugin-api.md");
            println!();
        }
        _ => {
            return Err(format!("Unknown plugin subcommand: '{}'. Use 'load', 'unload', or 'list'", args[0]));
        }
    }

    Ok(())
}

async fn cmd_webhook(args: &[&str], context: &ReplContext) -> Result<(), String> {
    use crate::webhook_manager::{WebhookManager, is_valid_event, print_webhook_events};

    // Initialize webhook manager (in production, this would be part of ReplContext)
    let home_dir = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let config_path = std::path::PathBuf::from(home_dir).join(".chiral/webhooks.json");
    let webhook_manager = WebhookManager::new(config_path);

    if args.is_empty() {
        return Err("Usage: webhook <add|remove|list|test>".to_string());
    }

    match args[0] {
        "add" => {
            if args.len() < 3 {
                return Err("Usage: webhook add <event> <url>".to_string());
            }

            let event = args[1];
            let url = args[2];

            // Validate event
            if !is_valid_event(event) {
                println!("\n{}", format!("[X] Invalid event: {}", event).red());
                print_webhook_events();
                return Ok(());
            }

            // Validate URL
            if url::Url::parse(url).is_err() {
                return Err(format!("Invalid URL: {}", url));
            }

            let webhook_id = webhook_manager.add_webhook(event.to_string(), url.to_string()).await?;

            println!("\n[OK] Webhook added successfully!");
            println!("  ┌──────────────────────────────────────────────────────────────┐");
            println!("  │ {:<60} │", format!("ID: {}", webhook_id));
            println!("  │ {:<60} │", format!("Event: {}", event));
            println!("  │ {:<60} │", format!("URL: {}", url));
            println!("  └──────────────────────────────────────────────────────────────┘");
            println!();
            println!("  Use {} to test the webhook", format!("webhook test {}", webhook_id).cyan());
            println!();
        }
        "remove" => {
            if args.len() < 2 {
                return Err("Usage: webhook remove <webhook_id>".to_string());
            }

            let webhook_id = args[1];
            webhook_manager.remove_webhook(webhook_id).await?;

            println!("\n[OK] Webhook removed: {}", webhook_id);
            println!();
        }
        "list" => {
            let webhooks = webhook_manager.list_webhooks().await;

            println!("\n[HOOK] Configured Webhooks:");
            println!("  ┌──────────────────────────────────────────────────────────────┐");

            if webhooks.is_empty() {
                println!("  │ {:<60} │", "No webhooks configured");
                println!("  │ {:<60} │", "");
                println!("  │ {:<60} │", "Add webhooks to receive event notifications");
            } else {
                println!("  │ {:<20} {:<15} {:<16} │", "Event", "Triggers", "Status");
                println!("  ├──────────────────────────────────────────────────────────────┤");

                for webhook in webhooks.iter().take(10) {
                    let status = if webhook.enabled { "Enabled" } else { "Disabled" };
                    println!("  │ {:<20} {:<15} {:<16} │",
                        &webhook.event[..webhook.event.len().min(20)],
                        webhook.trigger_count,
                        status
                    );
                    println!("  │ {:<60} │", format!("  ID: {}", webhook.id));
                    println!("  │ {:<60} │", format!("  URL: {}", &webhook.url[..webhook.url.len().min(50)]));

                    if let Some(last_triggered) = webhook.last_triggered {
                        let dt = chrono::DateTime::from_timestamp(last_triggered as i64, 0)
                            .unwrap_or_default();
                        println!("  │ {:<60} │", format!("  Last: {}", dt.format("%Y-%m-%d %H:%M:%S")));
                    }

                    println!("  ├──────────────────────────────────────────────────────────────┤");
                }

                if webhooks.len() > 10 {
                    println!("  │ {:<60} │", format!("... and {} more", webhooks.len() - 10));
                }
            }

            println!("  └──────────────────────────────────────────────────────────────┘");
            println!();
            print_webhook_events();
            println!();
        }
        "test" => {
            if args.len() < 2 {
                return Err("Usage: webhook test <webhook_id>".to_string());
            }

            let webhook_id = args[1];

            println!("\n[HOOK] Testing webhook: {}", webhook_id);
            println!("  Sending test payload...");

            webhook_manager.test_webhook(webhook_id, &context.peer_id).await?;

            println!("  [OK] Test webhook sent successfully!");
            println!("  Check your webhook endpoint for the test payload");
            println!();
        }
        "events" => {
            println!("\n[HOOK] Available Webhook Events:");
            print_webhook_events();
            println!();
        }
        _ => {
            return Err(format!("Unknown webhook subcommand: '{}'. Use 'add', 'remove', 'list', 'test', or 'events'", args[0]));
        }
    }

    Ok(())
}

async fn cmd_report(args: &[&str], context: &ReplContext) -> Result<(), String> {
    let report_type = args.get(0).unwrap_or(&"summary");

    match *report_type {
        "summary" | "full" => {
            println!("\n[STATS] Network Report");
            println!("  ┌──────────────────────────────────────────────────────────────┐");
            println!("  │ {:<60} │", format!("Generated: {}", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")));
            println!("  │ {:<60} │", format!("Peer ID: {}...", &context.peer_id[..20]));
            println!("  ├──────────────────────────────────────────────────────────────┤");

            // Network stats
            let peers = context.dht_service.get_connected_peers().await;
            let metrics = context.dht_service.metrics_snapshot().await;

            println!("  │ {:<60} │", "Network Status:");
            println!("  │ {:<60} │", format!("  Connected Peers: {}", peers.len()));
            println!("  │ {:<60} │", format!("  Reachability: {:?}", metrics.reachability));
            println!("  │ {:<60} │", format!("  AutoNAT: {}", if metrics.autonat_enabled { "Enabled" } else { "Disabled" }));

            // File transfer stats
            if let Some(ft) = &context.file_transfer_service {
                let snapshot = ft.download_metrics_snapshot().await;
                println!("  ├──────────────────────────────────────────────────────────────┤");
                println!("  │ {:<60} │", "Download Statistics:");
                println!("  │ {:<60} │", format!("  Successful: {}", snapshot.total_success));
                println!("  │ {:<60} │", format!("  Failed: {}", snapshot.total_failures));
                println!("  │ {:<60} │", format!("  Retries: {}", snapshot.total_retries));
            }

            println!("  └──────────────────────────────────────────────────────────────┘");
            println!();

            if *report_type == "full" {
                println!("  Use {} to export detailed data", "export all --format json".cyan());
                println!();
            }
        }
        _ => {
            return Err(format!("Unknown report type: '{}'. Use 'summary' or 'full'", report_type));
        }
    }

    Ok(())
}
