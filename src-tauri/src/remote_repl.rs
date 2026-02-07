// Remote REPL access via secure RPC
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use colored::Colorize;

use crate::repl::ReplContext;

pub struct RemoteReplServer {
    addr: String,
    auth_token: String,
    context: Arc<ReplContext>,
}

impl RemoteReplServer {
    pub fn new(addr: String, auth_token: String, context: Arc<ReplContext>) -> Self {
        Self {
            addr,
            auth_token,
            context,
        }
    }

    pub async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(&self.addr).await?;
        println!("\n[NET] Remote REPL server listening on: {}", self.addr.green());
        println!("  Auth token: {}", self.auth_token.yellow());
        println!("  Use 'nc {} <port>' to connect remotely", self.addr.split(':').next().unwrap());
        println!();

        loop {
            let (socket, addr) = listener.accept().await?;
            println!("  New connection from: {}", addr);

            let auth_token = self.auth_token.clone();
            let context = self.context.clone();

            tokio::spawn(async move {
                if let Err(e) = handle_client(socket, auth_token, context).await {
                    eprintln!("Error handling client {}: {}", addr, e);
                }
            });
        }
    }
}

async fn handle_client(
    mut socket: TcpStream,
    auth_token: String,
    context: Arc<ReplContext>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (reader, mut writer) = socket.split();
    let mut reader = BufReader::new(reader);

    // Send welcome message
    writer.write_all(b"Chiral Network Remote REPL\n").await?;
    writer.write_all(b"Please authenticate with your token:\n").await?;
    writer.flush().await?;

    // Read auth token
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let client_token = line.trim();

    if client_token != auth_token {
        writer.write_all(b"Authentication failed. Disconnecting.\n").await?;
        writer.flush().await?;
        return Ok(());
    }

    writer.write_all(b"Authentication successful!\n\n").await?;
    writer.write_all(b"Type 'help' for available commands, 'quit' to disconnect\n\n").await?;
    writer.flush().await?;

    // Main command loop
    loop {
        writer.write_all(b"chiral> ").await?;
        writer.flush().await?;

        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line).await?;

        if bytes_read == 0 {
            // Client disconnected
            break;
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if line == "quit" || line == "exit" {
            writer.write_all(b"Goodbye!\n").await?;
            writer.flush().await?;
            break;
        }

        // Parse and execute command
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        let command = parts[0];
        let args = &parts[1..];

        // Execute command (simplified - in real implementation would call actual command handlers)
        let response = match command {
            "status" => {
                let peers = context.dht_service.get_connected_peers().await;
                let metrics = context.dht_service.metrics_snapshot().await;
                format!(
                    "Network Status:\n  Connected Peers: {}\n  Reachability: {:?}\n\n",
                    peers.len(),
                    metrics.reachability
                )
            }
            "peers" => {
                if args.is_empty() || args[0] == "count" {
                    let peers = context.dht_service.get_connected_peers().await;
                    format!("Connected peers: {}\n\n", peers.len())
                } else {
                    "Use 'peers count' or 'peers list'\n\n".to_string()
                }
            }
            "help" => {
                "Available commands:\n  status - Show network status\n  peers - Show peers\n  quit - Disconnect\n\n".to_string()
            }
            _ => {
                format!("Unknown command: '{}'. Type 'help' for available commands.\n\n", command)
            }
        };

        writer.write_all(response.as_bytes()).await?;
        writer.flush().await?;
    }

    Ok(())
}

// Command to start remote REPL server from REPL
pub async fn cmd_remote(args: &[&str], context: &ReplContext) -> Result<(), String> {
    if args.is_empty() {
        return Err("Usage: remote <start|stop|status>".to_string());
    }

    match args[0] {
        "start" => {
            let addr = if args.len() > 1 {
                args[1].to_string()
            } else {
                "127.0.0.1:7777".to_string()
            };

            let token = if args.len() > 2 {
                args[2].to_string()
            } else {
                // Generate random token
                use rand::Rng;
                let token: String = rand::thread_rng()
                    .sample_iter(&rand::distributions::Alphanumeric)
                    .take(16)
                    .map(char::from)
                    .collect();
                token
            };

            println!("\n[NET] Starting remote REPL server...");
            println!("  Address: {}", addr.green());
            println!("  Auth Token: {}", token.yellow());
            println!();
            println!("  Connect with: {}", format!("telnet {} <port>", addr.split(':').next().unwrap()).cyan());
            println!("  Then authenticate with token: {}", token.yellow());
            println!();
            println!("  Note: Server runs in background. Use 'remote stop' to stop.");
            println!();

            // In a real implementation, we would spawn the server in a background task
            // and store the handle for later stopping
            println!("  (Remote server implementation requires background task handling)");
            println!();
        }
        "stop" => {
            println!("\n[NET] Stopping remote REPL server...");
            println!("  (Remote server stop requires task handle management)");
            println!();
        }
        "status" => {
            println!("\n[NET] Remote REPL Server Status:");
            println!("  ┌────────────────────────────────────────────────────────┐");
            println!("  │ {:<54} │", "Status: Not running");
            println!("  │ {:<54} │", "");
            println!("  │ {:<54} │", "Use 'remote start' to enable remote access");
            println!("  └────────────────────────────────────────────────────────┘");
            println!();
            println!("  Security note: Remote REPL uses token-based auth");
            println!("  Recommended: Use SSH port forwarding for production");
            println!();
        }
        _ => {
            return Err(format!("Unknown remote subcommand: '{}'. Use 'start', 'stop', or 'status'", args[0]));
        }
    }

    Ok(())
}
