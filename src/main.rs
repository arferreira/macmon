use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
    Frame, Terminal,
};
use sysinfo::{System, Disks, ProcessesToUpdate};
use std::{
    error::Error,
    io,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use walkdir::WalkDir;

#[derive(Clone)]
struct NodeModulesEntry {
    path: PathBuf,
    size: u64,
}

#[derive(Clone)]
struct DockerImage {
    name: String,
    size: String,
    created: String,
}

#[derive(Clone)]
struct TopProcess {
    name: String,
    cpu: f32,
    memory: u64,
    pid: u32,
}

#[derive(Clone)]
struct IssuesData {
    node_modules: Vec<NodeModulesEntry>,
    docker_images: Vec<DockerImage>,
    top_processes: Vec<TopProcess>,
    scanning: bool,
}

impl Default for IssuesData {
    fn default() -> Self {
        Self {
            node_modules: Vec::new(),
            docker_images: Vec::new(),
            top_processes: Vec::new(),
            scanning: true,
        }
    }
}

enum AppMode {
    Normal,
    CleanupMenu { selected: usize },
    KillProcessMenu { selected: usize },
}

struct App {
    system: System,
    disks: Disks,
    last_update: Instant,
    issues: Arc<Mutex<IssuesData>>,
    mode: AppMode,
}

impl App {
    fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_all();
        
        let issues = Arc::new(Mutex::new(IssuesData::default()));
        
        let issues_clone = Arc::clone(&issues);
        thread::spawn(move || {
            scan_issues(issues_clone);
        });

        Self {
            system,
            disks: Disks::new_with_refreshed_list(),
            last_update: Instant::now(),
            issues,
            mode: AppMode::Normal,
        }
    }

    fn update(&mut self) {
        if self.last_update.elapsed() >= Duration::from_secs(2) {
            self.system.refresh_all();
            self.disks.refresh(true);
            self.update_top_processes();
            self.last_update = Instant::now();
        }
    }

    fn update_top_processes(&mut self) {
        self.system.refresh_processes(ProcessesToUpdate::All, true);
        
        let mut processes: Vec<TopProcess> = self.system.processes()
            .values()
            .map(|p| TopProcess {
                name: p.name().to_string_lossy().to_string(),
                cpu: p.cpu_usage(),
                memory: p.memory(),
                pid: p.pid().as_u32(),
            })
            .collect();

        processes.sort_by(|a, b| {
            let a_score = a.cpu + (a.memory as f32 / 1_000_000.0);
            let b_score = b.cpu + (b.memory as f32 / 1_000_000.0);
            b_score.partial_cmp(&a_score).unwrap()
        });

        if let Ok(mut issues) = self.issues.lock() {
            issues.top_processes = processes.into_iter().take(5).collect();
        }
    }

    fn disk_usage(&self) -> (u64, u64, f64) {
        let mut total = 0;
        let mut used = 0;

        for disk in &self.disks {
            total += disk.total_space();
            used += disk.total_space() - disk.available_space();
        }

        let percentage = if total > 0 {
            (used as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        (used, total, percentage)
    }

    fn memory_usage(&self) -> (u64, u64, f64) {
        let total = self.system.total_memory();
        let used = self.system.used_memory();
        let percentage = (used as f64 / total as f64) * 100.0;
        (used, total, percentage)
    }

    fn cpu_usage(&self) -> f64 {
        self.system.global_cpu_usage() as f64
    }

    fn swap_usage(&self) -> (u64, u64, f64) {
        let total = self.system.total_swap();
        let used = self.system.used_swap();
        let percentage = if total > 0 {
            (used as f64 / total as f64) * 100.0
        } else {
            0.0
        };
        (used, total, percentage)
    }
}

fn scan_issues(issues: Arc<Mutex<IssuesData>>) {
    let home_dir = std::env::var("HOME").unwrap_or_else(|_| "/Users".to_string());
    
    let mut node_modules = scan_node_modules(&home_dir);
    node_modules.sort_by(|a, b| b.size.cmp(&a.size));
    node_modules.truncate(10);

    let docker_images = scan_docker_images();

    if let Ok(mut data) = issues.lock() {
        data.node_modules = node_modules;
        data.docker_images = docker_images;
        data.scanning = false;
    }
}

fn scan_node_modules(base_path: &str) -> Vec<NodeModulesEntry> {
    let mut results = Vec::new();
    let max_depth = 6;

    for entry in WalkDir::new(base_path)
        .max_depth(max_depth)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !name.starts_with('.') && 
            name != "Library" && 
            name != "System" &&
            name != "Applications"
        })
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_dir() && entry.file_name() == "node_modules" {
            if let Ok(size) = calculate_dir_size(entry.path()) {
                if size > 100_000_000 {
                    results.push(NodeModulesEntry {
                        path: entry.path().to_path_buf(),
                        size,
                    });
                }
            }
        }
    }

    results
}

fn calculate_dir_size(path: &std::path::Path) -> Result<u64, io::Error> {
    let mut total = 0;
    
    for entry in WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            if let Ok(metadata) = entry.metadata() {
                total += metadata.len();
            }
        }
    }

    Ok(total)
}

fn scan_docker_images() -> Vec<DockerImage> {
    let output = std::process::Command::new("docker")
        .args(["images", "--format", "{{.Repository}}:{{.Tag}}\t{{.Size}}\t{{.CreatedAt}}"])
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            return stdout
                .lines()
                .filter(|line| !line.is_empty())
                .filter_map(|line| {
                    let parts: Vec<&str> = line.split('\t').collect();
                    if parts.len() >= 3 {
                        Some(DockerImage {
                            name: parts[0].to_string(),
                            size: parts[1].to_string(),
                            created: parts[2].to_string(),
                        })
                    } else {
                        None
                    }
                })
                .take(10)
                .collect();
        }
    }

    Vec::new()
}

fn main() -> Result<(), Box<dyn Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let res = run_app(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{err:?}");
    }

    Ok(())
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> io::Result<()> {
    loop {
        app.update();
        terminal.draw(|f| ui(f, app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match &app.mode {
                    AppMode::Normal => match key.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Char('c') => {
                            app.mode = AppMode::CleanupMenu { selected: 0 };
                        },
                        _ => {}
                    },
                    AppMode::CleanupMenu { selected } => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            app.mode = AppMode::Normal;
                        },
                        KeyCode::Up | KeyCode::Char('k') => {
                            let new_selected = if *selected > 0 { selected - 1 } else { 3 };
                            app.mode = AppMode::CleanupMenu { selected: new_selected };
                        },
                        KeyCode::Down | KeyCode::Char('j') => {
                            let new_selected = if *selected < 3 { selected + 1 } else { 0 };
                            app.mode = AppMode::CleanupMenu { selected: new_selected };
                        },
                        KeyCode::Enter => {
                            if *selected == 3 {
                                app.mode = AppMode::KillProcessMenu { selected: 0 };
                            } else {
                                execute_cleanup(app, *selected)?;
                                app.mode = AppMode::Normal;
                            }
                        },
                        _ => {}
                    },
                    AppMode::KillProcessMenu { selected } => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            app.mode = AppMode::CleanupMenu { selected: 0 };
                        },
                        KeyCode::Up | KeyCode::Char('k') => {
                            let issues = app.issues.lock().unwrap();
                            let max = issues.top_processes.len().saturating_sub(1);
                            let new_selected = if *selected > 0 { selected - 1 } else { max };
                            app.mode = AppMode::KillProcessMenu { selected: new_selected };
                        },
                        KeyCode::Down | KeyCode::Char('j') => {
                            let issues = app.issues.lock().unwrap();
                            let max = issues.top_processes.len().saturating_sub(1);
                            let new_selected = if *selected < max { selected + 1 } else { 0 };
                            app.mode = AppMode::KillProcessMenu { selected: new_selected };
                        },
                        KeyCode::Enter => {
                            kill_process(app, *selected)?;
                            app.mode = AppMode::CleanupMenu { selected: 0 };
                        },
                        _ => {}
                    },
                }
            }
        }
    }
}

fn execute_cleanup(app: &App, option: usize) -> io::Result<()> {
    let issues = app.issues.lock().unwrap();
    
    match option {
        0 => {
            for nm in &issues.node_modules {
                let _ = std::fs::remove_dir_all(&nm.path);
            }
        },
        1 => {
            let _ = std::process::Command::new("docker")
                .args(["image", "prune", "-af"])
                .output();
        },
        2 => {
            let _ = std::process::Command::new("brew")
                .args(["cleanup", "-s"])
                .output();
        },
        _ => {}
    }
    
    Ok(())
}

fn kill_process(app: &App, index: usize) -> io::Result<()> {
    let issues = app.issues.lock().unwrap();
    
    if let Some(process) = issues.top_processes.get(index) {
        let _ = std::process::Command::new("kill")
            .arg("-9")
            .arg(process.pid.to_string())
            .output();
    }
    
    Ok(())
}

fn ui(f: &mut Frame, app: &App) {
    match &app.mode {
        AppMode::Normal => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(10),
                    Constraint::Min(5),
                    Constraint::Length(3),
                ])
                .split(f.area());

            render_metrics(f, app, chunks[0]);
            render_issues(f, app, chunks[1]);
            render_help(f, chunks[2]);
        },
        AppMode::CleanupMenu { selected } => {
            render_cleanup_menu(f, app, *selected);
        },
        AppMode::KillProcessMenu { selected } => {
            render_kill_process_menu(f, app, *selected);
        }
    }
}

fn render_cleanup_menu(f: &mut Frame, app: &App, selected: usize) {
    let issues = app.issues.lock().unwrap();
    
    let area = f.area();
    let popup_area = centered_rect(60, 50, area);
    
    f.render_widget(Block::default().style(Style::default().bg(Color::Black)), area);
    
    let block = Block::default()
        .title("Cleanup Menu")
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black));
    
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);
    
    let mut items = Vec::new();
    
    let nm_total: u64 = issues.node_modules.iter().map(|nm| nm.size).sum();
    let nm_text = format!("Clean {} node_modules ({:.1}GB)", 
        issues.node_modules.len(), 
        bytes_to_gb(nm_total)
    );
    let nm_style = if selected == 0 {
        Style::default().fg(Color::Black).bg(Color::White)
    } else {
        Style::default().fg(Color::White)
    };
    items.push(ListItem::new(nm_text).style(nm_style));
    
    let docker_text = format!("Prune Docker images ({})", issues.docker_images.len());
    let docker_style = if selected == 1 {
        Style::default().fg(Color::Black).bg(Color::White)
    } else {
        Style::default().fg(Color::White)
    };
    items.push(ListItem::new(docker_text).style(docker_style));
    
    let brew_text = "Clean Homebrew cache".to_string();
    let brew_style = if selected == 2 {
        Style::default().fg(Color::Black).bg(Color::White)
    } else {
        Style::default().fg(Color::White)
    };
    items.push(ListItem::new(brew_text).style(brew_style));
    
    let kill_text = "Kill heavy processes (free RAM)".to_string();
    let kill_style = if selected == 3 {
        Style::default().fg(Color::Black).bg(Color::White)
    } else {
        Style::default().fg(Color::White)
    };
    items.push(ListItem::new(kill_text).style(kill_style));
    
    items.push(ListItem::new(""));
    items.push(ListItem::new("[↑/↓] Navigate  [Enter] Execute  [Esc] Cancel")
        .style(Style::default().fg(Color::Gray)));
    
    let list = List::new(items);
    f.render_widget(list, inner);
}

fn render_kill_process_menu(f: &mut Frame, app: &App, selected: usize) {
    let issues = app.issues.lock().unwrap();
    
    let area = f.area();
    let popup_area = centered_rect(70, 60, area);
    
    f.render_widget(Block::default().style(Style::default().bg(Color::Black)), area);
    
    let block = Block::default()
        .title("Kill Process (Free RAM)")
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black));
    
    let inner = block.inner(popup_area);
    f.render_widget(block, popup_area);
    
    let mut items = Vec::new();
    
    items.push(ListItem::new("Select a process to kill:")
        .style(Style::default().fg(Color::Yellow)));
    items.push(ListItem::new(""));
    
    for (i, proc) in issues.top_processes.iter().enumerate() {
        let text = format!(
            "{} - {} (CPU: {:.1}%, RAM: {:.1}GB, PID: {})",
            i + 1,
            proc.name,
            proc.cpu,
            bytes_to_gb(proc.memory),
            proc.pid
        );
        
        let style = if i == selected {
            Style::default().fg(Color::Black).bg(Color::Red)
        } else {
            Style::default().fg(Color::White)
        };
        
        items.push(ListItem::new(text).style(style));
    }
    
    items.push(ListItem::new(""));
    items.push(ListItem::new("WARNING: This will force kill the process!")
        .style(Style::default().fg(Color::Red)));
    items.push(ListItem::new("[↑/↓] Navigate  [Enter] Kill  [Esc] Back")
        .style(Style::default().fg(Color::Gray)));
    
    let list = List::new(items);
    f.render_widget(list, inner);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn render_metrics(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title("Mac Health Monitor")
        .borders(Borders::ALL);
    
    let inner = block.inner(area);
    f.render_widget(block, area);

    let metrics_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
        ])
        .split(inner);

    let (disk_used, disk_total, disk_percent) = app.disk_usage();
    let disk_status = get_status_indicator(disk_percent);
    let disk_gauge = Gauge::default()
        .block(Block::default())
        .gauge_style(get_gauge_style(disk_percent))
        .label(format!(
            "Disk: {:.0}% ({:.1}GB/{:.1}GB) {}",
            disk_percent,
            bytes_to_gb(disk_used),
            bytes_to_gb(disk_total),
            disk_status
        ))
        .ratio(disk_percent / 100.0);
    f.render_widget(disk_gauge, metrics_layout[0]);

    let (mem_used, mem_total, mem_percent) = app.memory_usage();
    let mem_status = get_status_indicator(mem_percent);
    let mem_gauge = Gauge::default()
        .gauge_style(get_gauge_style(mem_percent))
        .label(format!(
            "RAM:  {:.0}% ({:.1}GB/{:.1}GB) {}",
            mem_percent,
            bytes_to_gb(mem_used),
            bytes_to_gb(mem_total),
            mem_status
        ))
        .ratio(mem_percent / 100.0);
    f.render_widget(mem_gauge, metrics_layout[1]);

    let cpu_percent = app.cpu_usage();
    let cpu_status = get_status_indicator(cpu_percent);
    let cpu_gauge = Gauge::default()
        .gauge_style(get_gauge_style(cpu_percent))
        .label(format!(
            "CPU:  {:.0}% avg {}",
            cpu_percent,
            cpu_status
        ))
        .ratio(cpu_percent / 100.0);
    f.render_widget(cpu_gauge, metrics_layout[2]);

    let (swap_used, _swap_total, swap_percent) = app.swap_usage();
    let swap_status = get_status_indicator(swap_percent);
    let swap_gauge = Gauge::default()
        .gauge_style(get_gauge_style(swap_percent))
        .label(format!(
            "Swap: {:.1}GB {}",
            bytes_to_gb(swap_used),
            swap_status
        ))
        .ratio(swap_percent / 100.0);
    f.render_widget(swap_gauge, metrics_layout[3]);
}

fn render_issues(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title("Top Issues")
        .borders(Borders::ALL);

    let issues = app.issues.lock().unwrap();
    
    let mut items = Vec::new();

    if issues.scanning {
        items.push(ListItem::new("Scanning filesystem...").style(Style::default().fg(Color::Yellow)));
    } else {
        if !issues.node_modules.is_empty() {
            let total_size: u64 = issues.node_modules.iter().map(|nm| nm.size).sum();
            items.push(ListItem::new(format!(
                "• node_modules: {:.1}GB in {} projects",
                bytes_to_gb(total_size),
                issues.node_modules.len()
            )).style(Style::default().fg(Color::Red)));

            for (i, nm) in issues.node_modules.iter().take(3).enumerate() {
                items.push(ListItem::new(format!(
                    "  {}. {} ({:.1}GB)",
                    i + 1,
                    nm.path.display(),
                    bytes_to_gb(nm.size)
                )));
            }
        }

        if !issues.docker_images.is_empty() {
            items.push(ListItem::new(format!(
                "• Docker images: {} found",
                issues.docker_images.len()
            )).style(Style::default().fg(Color::Yellow)));
            
            for img in issues.docker_images.iter().take(3) {
                items.push(ListItem::new(format!(
                    "  - {} ({})",
                    img.name,
                    img.size
                )));
            }
        }

        if !issues.top_processes.is_empty() {
            items.push(ListItem::new("• Top processes by resource usage:").style(Style::default().fg(Color::Cyan)));
            
            for proc in issues.top_processes.iter().take(3) {
                items.push(ListItem::new(format!(
                    "  - {} (CPU: {:.1}%, RAM: {:.1}GB)",
                    proc.name,
                    proc.cpu,
                    bytes_to_gb(proc.memory)
                )));
            }
        }

        if items.is_empty() {
            items.push(ListItem::new("No issues found!").style(Style::default().fg(Color::Green)));
        }
    }

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn render_help(f: &mut Frame, area: Rect) {
    let help_text = Paragraph::new("[c] Clean  [q] Quit")
        .style(Style::default().fg(Color::Gray))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(help_text, area);
}

fn get_status_indicator(percent: f64) -> &'static str {
    if percent >= 80.0 {
        "⚠️"
    } else if percent >= 60.0 {
        "⚡"
    } else {
        "✓"
    }
}

fn get_gauge_style(percent: f64) -> Style {
    let color = if percent >= 80.0 {
        Color::Red
    } else if percent >= 60.0 {
        Color::Yellow
    } else {
        Color::Green
    };
    Style::default().fg(color)
}

fn bytes_to_gb(bytes: u64) -> f64 {
    bytes as f64 / 1_073_741_824.0
}
