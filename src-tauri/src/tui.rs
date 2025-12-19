// TUI mode for live monitoring dashboard
use crate::dht::DhtService;
use crate::ethereum::GethProcess;
use crate::file_transfer::FileTransferService;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Gauge, List, ListItem, Paragraph, Tabs,
    },
    Frame, Terminal,
};
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct TuiContext {
    pub dht_service: Arc<DhtService>,
    pub file_transfer_service: Option<Arc<FileTransferService>>,
    pub geth_process: Option<GethProcess>,
    pub peer_id: String,
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
}

impl TuiState {
    fn new() -> Self {
        Self {
            active_panel: ActivePanel::Network,
            should_quit: false,
            last_update: Instant::now(),
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
    loop {
        terminal.draw(|f| ui(f, state, context))?;

        // Poll for events with timeout
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                handle_key_event(key, state);
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
    match key.code {
        KeyCode::Char('q') | KeyCode::Char('Q') => {
            state.should_quit = true;
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

fn ui(f: &mut Frame, state: &TuiState, context: &TuiContext) {
    let size = f.area();

    // Main layout: header + content
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Min(0),     // Content
            Constraint::Length(3),  // Footer
        ])
        .split(size);

    // Header
    render_header(f, chunks[0], context);

    // Content area with tabs
    render_content(f, chunks[1], state, context);

    // Footer
    render_footer(f, chunks[2]);
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

fn render_content(f: &mut Frame, area: Rect, state: &TuiState, context: &TuiContext) {
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
        ActivePanel::Network => render_network_panel(f, chunks[1], context),
        ActivePanel::Downloads => render_downloads_panel(f, chunks[1], context),
        ActivePanel::Peers => render_peers_panel(f, chunks[1], context),
        ActivePanel::Mining => render_mining_panel(f, chunks[1], context),
    }
}

fn render_network_panel(f: &mut Frame, area: Rect, context: &TuiContext) {
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

    // Network info (placeholder - would fetch real data)
    let network_info = vec![
        Line::from(vec![
            Span::styled("Connected Peers: ", Style::default().fg(Color::Gray)),
            Span::styled("42", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("Reachability: ", Style::default().fg(Color::Gray)),
            Span::styled("Public", Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("NAT Status: ", Style::default().fg(Color::Gray)),
            Span::styled("Active", Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("AutoNAT: ", Style::default().fg(Color::Gray)),
            Span::styled("Enabled", Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled("Circuit Relay: ", Style::default().fg(Color::Gray)),
            Span::styled("Connected", Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("DCUtR Success: ", Style::default().fg(Color::Gray)),
            Span::styled("85.2% (23/27)", Style::default().fg(Color::Cyan)),
        ]),
    ];

    let network_widget = Paragraph::new(network_info)
        .block(Block::default().borders(Borders::ALL).title("Network"));
    f.render_widget(network_widget, sections[0]);

    // DHT info
    let dht_info = vec![
        Line::from(vec![
            Span::styled("Reachability: ", Style::default().fg(Color::Gray)),
            Span::styled("Private", Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled("Confidence: ", Style::default().fg(Color::Gray)),
            Span::styled("Medium", Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled("DHT Entries: ", Style::default().fg(Color::Gray)),
            Span::styled("1,234", Style::default().fg(Color::Cyan)),
        ]),
    ];

    let dht_widget = Paragraph::new(dht_info)
        .block(Block::default().borders(Borders::ALL).title("DHT"));
    f.render_widget(dht_widget, sections[1]);

    // Transfer stats
    let stats_info = vec![
        Line::from(vec![
            Span::styled("Successful Downloads: ", Style::default().fg(Color::Gray)),
            Span::styled("156", Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("Failed Downloads: ", Style::default().fg(Color::Gray)),
            Span::styled("3", Style::default().fg(Color::Red)),
        ]),
        Line::from(vec![
            Span::styled("Upload Speed: ", Style::default().fg(Color::Gray)),
            Span::styled("4.2 MB/s", Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::styled("Download Speed: ", Style::default().fg(Color::Gray)),
            Span::styled("8.5 MB/s", Style::default().fg(Color::Cyan)),
        ]),
    ];

    let stats_widget = Paragraph::new(stats_info)
        .block(Block::default().borders(Borders::ALL).title("Transfer Stats"));
    f.render_widget(stats_widget, sections[2]);
}

fn render_downloads_panel(f: &mut Frame, area: Rect, context: &TuiContext) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("📥 Active Downloads");

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Mock download data
    let downloads = vec![
        ("document.pdf", 75, "8 peers", "4.2 MB/s", "2m 15s"),
        ("video.mp4", 30, "3 peers", "1.8 MB/s", "8m 42s"),
        ("archive.tar.gz", 92, "12 peers", "6.5 MB/s", "45s"),
    ];

    let download_height = 5; // Height per download item
    let constraints: Vec<Constraint> = downloads
        .iter()
        .map(|_| Constraint::Length(download_height))
        .chain(std::iter::once(Constraint::Min(0)))
        .collect();

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    for (i, (name, progress, peers, speed, eta)) in downloads.iter().enumerate() {
        let download_info = vec![
            Line::from(vec![
                Span::styled("File: ", Style::default().fg(Color::Gray)),
                Span::styled(*name, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled(*peers, Style::default().fg(Color::Yellow)),
                Span::raw("  │  "),
                Span::styled(*speed, Style::default().fg(Color::Green)),
                Span::raw("  │  ETA: "),
                Span::styled(*eta, Style::default().fg(Color::Magenta)),
            ]),
        ];

        let download_block = Block::default().borders(Borders::ALL);
        let download_inner = download_block.inner(sections[i]);
        f.render_widget(download_block, sections[i]);

        // Info
        let info_area = Rect {
            x: download_inner.x,
            y: download_inner.y,
            width: download_inner.width,
            height: 2,
        };
        let info_widget = Paragraph::new(download_info);
        f.render_widget(info_widget, info_area);

        // Progress bar
        let progress_area = Rect {
            x: download_inner.x,
            y: download_inner.y + 2,
            width: download_inner.width,
            height: 1,
        };
        let gauge = Gauge::default()
            .percent(*progress as u16)
            .label(format!("{}%", progress))
            .gauge_style(Style::default().fg(Color::Green));
        f.render_widget(gauge, progress_area);
    }

    if downloads.is_empty() {
        let no_downloads = Paragraph::new("No active downloads\n\nUse REPL mode to start downloads")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Gray));
        f.render_widget(no_downloads, inner);
    }
}

fn render_peers_panel(f: &mut Frame, area: Rect, context: &TuiContext) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title("👥 Connected Peers");

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Mock peer data
    let peers = vec![
        ("12D3KooW...AbCdEf", "82", "High", "45ms", "2.5 MB/s"),
        ("12D3KooW...XyZ123", "78", "Medium", "52ms", "1.8 MB/s"),
        ("12D3KooW...Qrs456", "91", "High", "38ms", "3.2 MB/s"),
        ("12D3KooW...Tuv789", "65", "Low", "120ms", "0.5 MB/s"),
        ("12D3KooW...Wxy012", "88", "High", "41ms", "2.9 MB/s"),
    ];

    let items: Vec<ListItem> = peers
        .iter()
        .map(|(id, score, trust, latency, bandwidth)| {
            let trust_color = match *trust {
                "High" => Color::Green,
                "Medium" => Color::Yellow,
                _ => Color::Red,
            };

            let content = vec![Line::from(vec![
                Span::styled(format!("{:<20}", id), Style::default().fg(Color::Cyan)),
                Span::raw("  "),
                Span::styled(format!("Score: {:<3}", score), Style::default().fg(Color::White)),
                Span::raw("  "),
                Span::styled(format!("{:<8}", trust), Style::default().fg(trust_color)),
                Span::raw("  "),
                Span::styled(format!("{:<8}", latency), Style::default().fg(Color::Magenta)),
                Span::raw("  "),
                Span::styled(*bandwidth, Style::default().fg(Color::Green)),
            ])];

            ListItem::new(content)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Peer List"));

    f.render_widget(list, inner);
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

fn render_footer(f: &mut Frame, area: Rect) {
    let help_text = vec![
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
