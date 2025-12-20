// TUI mode for live monitoring dashboard
use crate::dht::DhtService;
use crate::ethereum::GethProcess;
use crate::file_transfer::FileTransferService;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use hex;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, List, ListItem, Paragraph, Tabs,
    },
    Frame, Terminal,
};
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};
use crate::dht::models::DhtMetricsSnapshot;
use crate::file_transfer::DownloadMetricsSnapshot;

pub struct TuiContext {
    pub dht_service: Arc<DhtService>,
    pub file_transfer_service: Option<Arc<FileTransferService>>,
    pub geth_process: Option<GethProcess>,
    pub peer_id: String,
}

#[derive(Debug, Clone)]
struct LiveMetrics {
    connected_peers: Vec<String>,
    dht_metrics: DhtMetricsSnapshot,
    download_metrics: Option<DownloadMetricsSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ActivePanel {
    Network,
    Downloads,
    Peers,
    Mining,
}

struct TuiState {
    active_panel: ActivePanel,
    should_quit: bool,
    last_update: Instant,
    command_mode: bool,
    command_input: String,
    command_result: Option<(String, bool)>, // (message, is_error)
}

impl TuiState {
    fn new() -> Self {
        Self {
            active_panel: ActivePanel::Network,
            should_quit: false,
            last_update: Instant::now(),
            command_mode: false,
            command_input: String::new(),
            command_result: None,
        }
    }

    fn next_panel(&mut self) {
        self.active_panel = match self.active_panel {
            ActivePanel::Network => ActivePanel::Downloads,
            ActivePanel::Downloads => ActivePanel::Peers,
            ActivePanel::Peers => ActivePanel::Mining,
            ActivePanel::Mining => ActivePanel::Network,
        };
    }

    fn previous_panel(&mut self) {
        self.active_panel = match self.active_panel {
            ActivePanel::Network => ActivePanel::Mining,
            ActivePanel::Downloads => ActivePanel::Network,
            ActivePanel::Peers => ActivePanel::Downloads,
            ActivePanel::Mining => ActivePanel::Peers,
        };
    }

    fn select_panel(&mut self, index: usize) {
        self.active_panel = match index {
            0 => ActivePanel::Network,
            1 => ActivePanel::Downloads,
            2 => ActivePanel::Peers,
            3 => ActivePanel::Mining,
            _ => self.active_panel,
        };
    }
}

pub async fn run_tui(context: TuiContext) -> Result<(), Box<dyn std::error::Error>> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let mut state = TuiState::new();

    // Main loop
    let res = run_app(&mut terminal, &mut state, &context).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error: {:?}", err);
    }

    Ok(())
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    state: &mut TuiState,
    context: &TuiContext,
) -> io::Result<()> {
    // Create a channel for live metrics updates
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    // Clone Arc references for the background task
    let dht_clone = context.dht_service.clone();
    let ft_clone = context.file_transfer_service.clone();

    tokio::spawn(async move {
        loop {
            // Fetch metrics from services
            let connected_peers = dht_clone.get_connected_peers().await;
            let dht_metrics = dht_clone.metrics_snapshot().await;
            let download_metrics = if let Some(ft) = &ft_clone {
                Some(ft.download_metrics_snapshot().await)
            } else {
                None
            };

            let metrics = LiveMetrics {
                connected_peers,
                dht_metrics,
                download_metrics,
            };

            let _ = tx.send(metrics);
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    let mut current_metrics: Option<LiveMetrics> = None;
    let mut pending_command: Option<String> = None;

    loop {
        // Check for new metrics
        while let Ok(metrics) = rx.try_recv() {
            current_metrics = Some(metrics);
        }

        // Execute pending command if any
        if let Some(cmd) = pending_command.take() {
            if !cmd.is_empty() {
                match execute_command(&cmd, context).await {
                    Ok(result) => {
                        state.command_result = Some((result, false));
                    }
                    Err(err) => {
                        state.command_result = Some((err, true));
                    }
                }
            }
        }

        terminal.draw(|f| ui(f, state, context, &current_metrics))?;

        // Poll for events with timeout
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                let was_in_command_mode = state.command_mode;
                handle_key_event(key, state);

                // If we just exited command mode with Enter, execute the command
                if was_in_command_mode && !state.command_mode && key.code == KeyCode::Enter {
                    pending_command = Some(state.command_input.clone());
                    state.command_input.clear();
                }
            }
        }

        // Auto-refresh every second
        if state.last_update.elapsed() >= Duration::from_secs(1) {
            state.last_update = Instant::now();
        }

        if state.should_quit {
            return Ok(());
        }
    }
}

fn handle_key_event(key: KeyEvent, state: &mut TuiState) {
    if state.command_mode {
        // In command mode - handle text input
        match key.code {
            KeyCode::Char(c) => {
                state.command_input.push(c);
            }
            KeyCode::Backspace => {
                state.command_input.pop();
            }
            KeyCode::Enter => {
                // Command will be executed in the async context
                // Just mark that we're done editing
                state.command_mode = false;
            }
            KeyCode::Esc => {
                // Cancel command mode
                state.command_mode = false;
                state.command_input.clear();
                state.command_result = None;
            }
            _ => {}
        }
    } else {
        // Normal mode - handle navigation and commands
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => {
                state.should_quit = true;
            }
            KeyCode::Char(':') => {
                // Enter command mode
                state.command_mode = true;
                state.command_input.clear();
                state.command_result = None;
            }
            KeyCode::Char('1') => state.select_panel(0),
            KeyCode::Char('2') => state.select_panel(1),
            KeyCode::Char('3') => state.select_panel(2),
            KeyCode::Char('4') => state.select_panel(3),
            KeyCode::Tab => state.next_panel(),
            KeyCode::BackTab => state.previous_panel(),
            KeyCode::Right => state.next_panel(),
            KeyCode::Left => state.previous_panel(),
            _ => {}
        }
    }
}

fn ui(f: &mut Frame, state: &TuiState, context: &TuiContext, metrics: &Option<LiveMetrics>) {
    let size = f.area();

    // Main layout: header + content + command result (if any) + footer
    let footer_height = if state.command_result.is_some() { 5 } else { 3 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),        // Header
            Constraint::Min(0),           // Content
            Constraint::Length(footer_height),  // Footer (includes command result)
        ])
        .split(size);

    // Header
    render_header(f, chunks[0], context);

    // Content area with tabs
    render_content(f, chunks[1], state, context, metrics);

    // Footer (includes command bar and results)
    render_footer(f, chunks[2], state);
}

fn render_header(f: &mut Frame, area: Rect, context: &TuiContext) {
    let title = vec![
        Span::styled(
            "Chiral Network",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" v0.1.0 - TUI Dashboard"),
    ];

    let peer_id_short = if context.peer_id.len() > 20 {
        format!("{}...{}", &context.peer_id[..8], &context.peer_id[context.peer_id.len()-8..])
    } else {
        context.peer_id.clone()
    };

    let subtitle = format!("Peer ID: {}", peer_id_short);

    let header = Paragraph::new(vec![
        Line::from(title),
        Line::from(subtitle),
    ])
    .block(Block::default().borders(Borders::ALL))
    .alignment(Alignment::Center);

    f.render_widget(header, area);
}

fn render_content(f: &mut Frame, area: Rect, state: &TuiState, context: &TuiContext, metrics: &Option<LiveMetrics>) {
    // Split into tabs and panel area
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Tabs
            Constraint::Min(0),     // Panel content
        ])
        .split(area);

    // Tabs
    let titles = vec!["Network [1]", "Downloads [2]", "Peers [3]", "Mining [4]"];
    let selected_index = match state.active_panel {
        ActivePanel::Network => 0,
        ActivePanel::Downloads => 1,
        ActivePanel::Peers => 2,
        ActivePanel::Mining => 3,
    };

    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title("Panels"))
        .select(selected_index)
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

    f.render_widget(tabs, chunks[0]);

    // Render active panel
    match state.active_panel {
        ActivePanel::Network => render_network_panel(f, chunks[1], context, metrics),
        ActivePanel::Downloads => render_downloads_panel(f, chunks[1], context, metrics),
        ActivePanel::Peers => render_peers_panel(f, chunks[1], context, metrics),
        ActivePanel::Mining => render_mining_panel(f, chunks[1], context),
    }
}

fn render_network_panel(f: &mut Frame, area: Rect, context: &TuiContext, metrics: &Option<LiveMetrics>) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("📡 Network Status");

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Split into sections
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),   // Network info
            Constraint::Length(6),   // DHT info
            Constraint::Min(0),      // Stats
        ])
        .split(inner);

    // Get real data from metrics or show loading
    let (peer_count, reachability, nat_status, autonat_status, relay_status, dcutr_stats) =
        if let Some(m) = metrics {
            let peer_count = m.connected_peers.len();
            let reachability = format!("{:?}", m.dht_metrics.reachability);
            let nat_status = if m.dht_metrics.observed_addrs.is_empty() {
                "Unknown".to_string()
            } else {
                "Active".to_string()
            };
            let autonat_status = if m.dht_metrics.autonat_enabled {
                "Enabled".to_string()
            } else {
                "Disabled".to_string()
            };
            let relay_status = if let Some(relay_id) = &m.dht_metrics.active_relay_peer_id {
                format!("Active ({}...)", &relay_id[..12])
            } else {
                "None".to_string()
            };
            let dcutr_stats = if m.dht_metrics.dcutr_enabled {
                let success_rate = if m.dht_metrics.dcutr_hole_punch_attempts > 0 {
                    (m.dht_metrics.dcutr_hole_punch_successes as f64 / m.dht_metrics.dcutr_hole_punch_attempts as f64) * 100.0
                } else {
                    0.0
                };
                format!("{:.1}% ({}/{})", success_rate, m.dht_metrics.dcutr_hole_punch_successes, m.dht_metrics.dcutr_hole_punch_attempts)
            } else {
                "Disabled".to_string()
            };
            (peer_count, reachability, nat_status, autonat_status, relay_status, dcutr_stats)
        } else {
            (0, "Loading...".to_string(), "Loading...".to_string(), "Loading...".to_string(), "Loading...".to_string(), "Loading...".to_string())
        };

    // Network info with real data
    let network_info = vec![
        Line::from(vec![
            Span::styled("Connected Peers: ", Style::default().fg(Color::Gray)),
            Span::styled(peer_count.to_string(), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("Reachability: ", Style::default().fg(Color::Gray)),
            Span::styled(reachability, Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("NAT Status: ", Style::default().fg(Color::Gray)),
            Span::styled(nat_status, Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("AutoNAT: ", Style::default().fg(Color::Gray)),
            Span::styled(autonat_status, Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled("Circuit Relay: ", Style::default().fg(Color::Gray)),
            Span::styled(relay_status, Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("DCUtR Success: ", Style::default().fg(Color::Gray)),
            Span::styled(dcutr_stats, Style::default().fg(Color::Cyan)),
        ]),
    ];

    let network_widget = Paragraph::new(network_info)
        .block(Block::default().borders(Borders::ALL).title("Network"));
    f.render_widget(network_widget, sections[0]);

    // DHT info with real data
    let (dht_reachability, dht_confidence, observed_count) = if let Some(m) = metrics {
        (
            format!("{:?}", m.dht_metrics.reachability),
            format!("{:?}", m.dht_metrics.reachability_confidence),
            m.dht_metrics.observed_addrs.len(),
        )
    } else {
        ("Loading...".to_string(), "Loading...".to_string(), 0)
    };

    let dht_info = vec![
        Line::from(vec![
            Span::styled("Reachability: ", Style::default().fg(Color::Gray)),
            Span::styled(dht_reachability, Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled("Confidence: ", Style::default().fg(Color::Gray)),
            Span::styled(dht_confidence, Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled("Observed Addresses: ", Style::default().fg(Color::Gray)),
            Span::styled(observed_count.to_string(), Style::default().fg(Color::Cyan)),
        ]),
    ];

    let dht_widget = Paragraph::new(dht_info)
        .block(Block::default().borders(Borders::ALL).title("DHT"));
    f.render_widget(dht_widget, sections[1]);

    // Transfer stats with real data
    let (success_count, fail_count, retry_count) = if let Some(m) = metrics {
        if let Some(dm) = &m.download_metrics {
            (dm.total_success, dm.total_failures, dm.total_retries)
        } else {
            (0, 0, 0)
        }
    } else {
        (0, 0, 0)
    };

    let stats_info = vec![
        Line::from(vec![
            Span::styled("Successful Downloads: ", Style::default().fg(Color::Gray)),
            Span::styled(success_count.to_string(), Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("Failed Downloads: ", Style::default().fg(Color::Gray)),
            Span::styled(fail_count.to_string(), Style::default().fg(Color::Red)),
        ]),
        Line::from(vec![
            Span::styled("Total Retries: ", Style::default().fg(Color::Gray)),
            Span::styled(retry_count.to_string(), Style::default().fg(Color::Yellow)),
        ]),
    ];

    let stats_widget = Paragraph::new(stats_info)
        .block(Block::default().borders(Borders::ALL).title("Transfer Stats"));
    f.render_widget(stats_widget, sections[2]);
}

fn render_downloads_panel(f: &mut Frame, area: Rect, context: &TuiContext, metrics: &Option<LiveMetrics>) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("📥 Recent Downloads");

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Get recent download attempts from real data
    let downloads = if let Some(m) = metrics {
        if let Some(dm) = &m.download_metrics {
            dm.recent_attempts.iter().take(10).map(|attempt| {
                let hash_short = if attempt.file_hash.len() > 16 {
                    format!("{}...{}", &attempt.file_hash[..8], &attempt.file_hash[attempt.file_hash.len()-4..])
                } else {
                    attempt.file_hash.clone()
                };
                let status = format!("{:?}", attempt.status);
                let attempts_str = format!("{}/{}", attempt.attempt, attempt.max_attempts);
                (hash_short, status, attempts_str)
            }).collect::<Vec<_>>()
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    if downloads.is_empty() {
        let no_downloads = Paragraph::new("No recent downloads\n\nUse REPL mode or GUI to start downloads")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Gray));
        f.render_widget(no_downloads, inner);
    } else {
        // Display as a list
        let items: Vec<ListItem> = downloads
            .iter()
            .map(|(hash, status, attempts)| {
                let status_color = if status.contains("Success") {
                    Color::Green
                } else if status.contains("Failed") {
                    Color::Red
                } else {
                    Color::Yellow
                };

                let content = vec![Line::from(vec![
                    Span::styled(format!("{:<20}", hash), Style::default().fg(Color::Cyan)),
                    Span::raw("  "),
                    Span::styled(format!("{:<12}", status), Style::default().fg(status_color)),
                    Span::raw("  "),
                    Span::styled(format!("Attempt: {}", attempts), Style::default().fg(Color::Gray)),
                ])];

                ListItem::new(content)
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Recent Attempts"));

        f.render_widget(list, inner);
    }
}

fn render_peers_panel(f: &mut Frame, area: Rect, context: &TuiContext, metrics: &Option<LiveMetrics>) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("👥 Connected Peers");

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Get real peer data
    let peers = if let Some(m) = metrics {
        m.connected_peers.iter().take(20).map(|peer_id| {
            let peer_short = if peer_id.len() > 20 {
                format!("{}...{}", &peer_id[..8], &peer_id[peer_id.len()-8..])
            } else {
                peer_id.clone()
            };
            peer_short
        }).collect::<Vec<_>>()
    } else {
        vec![]
    };

    if peers.is_empty() {
        let no_peers = Paragraph::new("No connected peers\n\nConnecting to network...")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Gray));
        f.render_widget(no_peers, inner);
    } else {
        let items: Vec<ListItem> = peers
            .iter()
            .enumerate()
            .map(|(i, peer_id)| {
                let content = vec![Line::from(vec![
                    Span::styled(format!("{:>3}. ", i + 1), Style::default().fg(Color::Gray)),
                    Span::styled(peer_id, Style::default().fg(Color::Cyan)),
                ])];

                ListItem::new(content)
            })
            .collect();

        let peer_count_title = format!("Peer List ({} connected)", peers.len());
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(peer_count_title));

        f.render_widget(list, inner);
    }
}

fn render_mining_panel(f: &mut Frame, area: Rect, context: &TuiContext) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("⛏️  Mining Status");

    let inner = block.inner(area);
    f.render_widget(block, area);

    if context.geth_process.is_none() {
        let no_mining = Paragraph::new(
            "Mining not enabled\n\nStart with --enable-geth flag to enable mining"
        )
        .alignment(Alignment::Center)
        .style(Style::default().fg(Color::Gray));
        f.render_widget(no_mining, inner);
        return;
    }

    // Split into sections
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8),   // Mining info
            Constraint::Min(0),      // Recent blocks
        ])
        .split(inner);

    // Mining info
    let mining_info = vec![
        Line::from(vec![
            Span::styled("Status: ", Style::default().fg(Color::Gray)),
            Span::styled("Active", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("Hash Rate: ", Style::default().fg(Color::Gray)),
            Span::styled("234 MH/s", Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("Threads: ", Style::default().fg(Color::Gray)),
            Span::styled("4", Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled("Blocks Found: ", Style::default().fg(Color::Gray)),
            Span::styled("12", Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("Total Rewards: ", Style::default().fg(Color::Gray)),
            Span::styled("24.5 ETC", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("Power: ", Style::default().fg(Color::Gray)),
            Span::styled("~350W", Style::default().fg(Color::Magenta)),
        ]),
    ];

    let mining_widget = Paragraph::new(mining_info)
        .block(Block::default().borders(Borders::ALL).title("Mining Info"));
    f.render_widget(mining_widget, sections[0]);

    // Recent blocks
    let blocks = vec![
        "Block #1234567 - 2.0 ETC - 2 min ago",
        "Block #1234512 - 2.0 ETC - 15 min ago",
        "Block #1234489 - 2.0 ETC - 28 min ago",
    ];

    let block_items: Vec<ListItem> = blocks
        .iter()
        .map(|b| ListItem::new(Line::from(*b)))
        .collect();

    let block_list = List::new(block_items)
        .block(Block::default().borders(Borders::ALL).title("Recent Blocks"));

    f.render_widget(block_list, sections[1]);
}

fn render_footer(f: &mut Frame, area: Rect, state: &TuiState) {
    if state.command_mode {
        // Show command input mode
        let command_text = format!(":{}", state.command_input);
        let command_widget = Paragraph::new(command_text)
            .block(Block::default().borders(Borders::ALL).title("Command Mode"))
            .style(Style::default().fg(Color::Yellow));
        f.render_widget(command_widget, area);
    } else if let Some((result, is_error)) = &state.command_result {
        // Show command result
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),  // Result message (wrap if needed)
                Constraint::Length(1),  // Help text
            ])
            .split(area);

        let result_color = if *is_error { Color::Red } else { Color::Green };
        let result_prefix = if *is_error { "❌ " } else { "✓ " };
        let result_widget = Paragraph::new(format!("{}{}", result_prefix, result))
            .block(Block::default().borders(Borders::ALL))
            .style(Style::default().fg(result_color))
            .wrap(ratatui::widgets::Wrap { trim: false });
        f.render_widget(result_widget, sections[0]);

        let help_text = vec![
            Span::styled("[:]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" Command  "),
            Span::styled("[Q]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" Quit  "),
            Span::styled("[1-4]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" Panels"),
        ];
        let help_widget = Paragraph::new(Line::from(help_text))
            .alignment(Alignment::Center);
        f.render_widget(help_widget, sections[1]);
    } else {
        // Normal help text
        let help_text = vec![
            Span::styled("[:]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" Command  "),
            Span::styled("[Q]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" Quit  "),
            Span::styled("[1-4]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" Select Panel  "),
            Span::styled("[Tab]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" Next  "),
            Span::styled("[←→]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" Navigate"),
        ];

        let footer = Paragraph::new(Line::from(help_text))
            .block(Block::default().borders(Borders::ALL))
            .alignment(Alignment::Center);

        f.render_widget(footer, area);
    }
}

// Command execution
async fn execute_command(command: &str, context: &TuiContext) -> Result<String, String> {
    let parts: Vec<&str> = command.trim().split_whitespace().collect();
    if parts.is_empty() {
        return Err("Empty command".to_string());
    }

    let cmd = parts[0];
    let args = &parts[1..];

    match cmd {
        "downloads" => {
            if let Some(ft_service) = &context.file_transfer_service {
                let metrics = ft_service.download_metrics_snapshot().await;
                let mut result = format!("Download Metrics:\n  Success: {}\n  Failures: {}\n  Retries: {}\n\nRecent Attempts: {}\n",
                    metrics.total_success, metrics.total_failures, metrics.total_retries, metrics.recent_attempts.len());

                for attempt in metrics.recent_attempts.iter().take(5) {
                    result.push_str(&format!("  - {} [{:?}] attempt {}/{}\n",
                        &attempt.file_hash[..16], attempt.status, attempt.attempt, attempt.max_attempts));
                }
                Ok(result)
            } else {
                Err("File transfer service not available".to_string())
            }
        }
        "help" | "h" => {
            Ok("Commands:\n  add <path> - Add file (hash saved to /tmp/chiral_last_hash.txt)\n  download <hash|last> - Download by hash or 'last' for last added\n  downloads - Show download metrics and recent attempts\n  status - Node status\n  peers - Connected peers\n  dht status - DHT status\n  mining status - Mining status".to_string())
        }
        "status" | "s" => {
            let peers = context.dht_service.get_connected_peers().await;
            let metrics = context.dht_service.metrics_snapshot().await;
            Ok(format!("Peers: {}, Reachability: {:?}", peers.len(), metrics.reachability))
        }
        "peers" => {
            let peers = context.dht_service.get_connected_peers().await;
            Ok(format!("Connected peers: {}", peers.len()))
        }
        "add" => {
            if args.is_empty() {
                return Err("Usage: add <file_path>".to_string());
            }
            let file_path = args.join(" ");

            if !std::path::Path::new(&file_path).exists() {
                return Err(format!("File not found: {}", file_path));
            }

            let file_data = std::fs::read(&file_path)
                .map_err(|e| format!("Failed to read file: {}", e))?;

            use sha2::Digest;
            let mut hasher = sha2::Sha256::new();
            hasher.update(&file_data);
            let hash_bytes = hasher.finalize();
            let hash_hex = hex::encode(&hash_bytes);
            let hash = format!("Qm{}", hash_hex);

            let file_name = std::path::Path::new(&file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            use crate::dht::models::FileMetadata;
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

            context.dht_service.publish_file(metadata, None).await
                .map_err(|e| format!("Failed to publish: {}", e))?;

            // Save hash to file for easy copying
            let hash_file = "/tmp/chiral_last_hash.txt";
            std::fs::write(hash_file, &hash)
                .map_err(|e| format!("Warning: couldn't save hash to {}: {}", hash_file, e))?;

            Ok(format!("Added: {}\nHash: {}\nSaved to: {}", file_name, hash, hash_file))
        }
        "download" | "dl" => {
            if args.is_empty() {
                return Err("Usage: download <file_hash> (or 'last' for last added hash)".to_string());
            }
            let hash = if args[0] == "last" {
                // Read from saved hash file
                std::fs::read_to_string("/tmp/chiral_last_hash.txt")
                    .map_err(|_| "No saved hash found. Add a file first.".to_string())?
            } else {
                args[0].to_string()
            };

            // Initiate DHT search for peers
            context.dht_service.get_file(hash.clone()).await
                .map_err(|e| format!("DHT search failed: {}", e))?;

            // Also trigger actual download through file transfer service
            if let Some(ft_service) = &context.file_transfer_service {
                let output_path = format!("/tmp/download_{}", hash);
                ft_service.download_file_with_account(
                    hash.clone(),
                    output_path.clone(),
                    None,
                    None
                ).await
                    .map_err(|e| format!("Download initiation failed: {}", e))?;

                Ok(format!("Download started for: {}\nOutput: {}\nCheck Downloads panel for progress", hash, output_path))
            } else {
                Ok(format!("DHT search initiated for: {}\n(File transfer service not available)", hash))
            }
        }
        "dht" => {
            if args.is_empty() || args[0] != "status" {
                return Err("Usage: dht status".to_string());
            }
            let metrics = context.dht_service.metrics_snapshot().await;
            Ok(format!("Reachability: {:?}, Confidence: {:?}", metrics.reachability, metrics.reachability_confidence))
        }
        "mining" => {
            if context.geth_process.is_none() {
                return Err("Mining requires --enable-geth flag".to_string());
            }
            if args.is_empty() || args[0] != "status" {
                return Err("Usage: mining status (start/stop not supported in TUI yet)".to_string());
            }
            Ok("Mining status: (requires geth integration)".to_string())
        }
        _ => {
            Err(format!("Unknown command: '{}'. Type 'help' for available commands", cmd))
        }
    }
}
