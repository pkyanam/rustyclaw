use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};
use std::io;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::agent::Agent;
use crate::config::Config;
use crate::memory::Memory;
use crate::scheduler::Scheduler;
use crate::workspace::Workspace;

pub struct TuiApp {
    config: Config,
    agent: Arc<Agent>,
    memory: Arc<Memory>,
    scheduler: Arc<Scheduler>,
    workspace: Arc<Workspace>,
    messages: Vec<(String, bool)>,
    input: String,
    processing: bool,
    telegram_callback: Arc<RwLock<Option<Arc<dyn Fn(String) + Send + Sync>>>>,
}

impl TuiApp {
    pub fn new(
        config: Config,
        agent: Arc<Agent>,
        memory: Arc<Memory>,
        scheduler: Arc<Scheduler>,
        workspace: Arc<Workspace>,
    ) -> Self {
        Self {
            config,
            agent,
            memory,
            scheduler,
            workspace,
            messages: Vec::new(),
            input: String::new(),
            processing: false,
            telegram_callback: Arc::new(RwLock::new(None)),
        }
    }

    pub fn set_telegram_callback<F>(&self, callback: F)
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        let mut cb = self.telegram_callback.blocking_write();
        *cb = Some(Arc::new(callback));
    }

    async fn send_to_telegram(&self, message: &str) {
        let cb = self.telegram_callback.read().await;
        if let Some(callback) = cb.as_ref() {
            callback(message.to_string());
        }
    }

    fn add_message(&mut self, role: &str, content: &str) {
        let is_user = role == "user";
        self.messages.push((format!("{}: {}", if is_user { "You" } else { "RustyClaw" }, content), is_user));
    }

    fn add_status(&mut self, emoji: &str, message: &str) {
        self.messages.push((format!("{} {}", emoji, message), false));
    }

    async fn process_message(&mut self, user_text: String) {
        self.processing = true;
        self.add_message("user", &user_text);

        self.memory.add_message("user", &user_text).await.ok();
        
        let history = self.memory.get_history(self.config.memory.max_history).await.unwrap_or_default();

        let response = self.agent.chat(&history).await.unwrap_or_else(|e| {
            format!("Sorry, I had trouble thinking about that. Error: {}", e)
        });

        let (cron_jobs, cron_errors) = Agent::parse_cron_blocks(&response);
        
        for error in cron_errors {
            self.add_status("⚠️", &format!("Cron error: {}", error));
        }

        for job in cron_jobs {
            match self.scheduler.add_job(&job.schedule, &job.task, &job.message).await {
                Ok(job_id) => {
                    self.add_status("✅", &format!("Scheduled job #{}: {} ({})", job_id, job.task, job.schedule));
                }
                Err(e) => {
                    self.add_status("❌", &format!("Error scheduling: {}", e));
                }
            }
        }

        let save_blocks = Agent::parse_save_blocks(&response);
        for block in save_blocks {
            match self.workspace.save_file(&block.filename, &block.content).await {
                Ok(path) => {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or(&block.filename);
                    self.add_status("💾", &format!("Saved {} to workspace", name));
                }
                Err(e) => {
                    self.add_status("❌", &format!("Error saving file: {}", e));
                }
            }
        }

        let memory_blocks = Agent::parse_memory_blocks(&response);
        for fact in memory_blocks {
            if self.agent.save_to_memory(&fact).await.unwrap_or(false) {
                self.add_status("🧠", &format!("Remembered: {}", fact));
            }
        }

        let clean = Agent::clean_response(&response);
        if !clean.is_empty() {
            self.add_message("assistant", &clean);
        }

        self.memory.add_message("assistant", &response).await.ok();

        self.send_to_telegram(&format!("💻 TUI: {}\n\n{}", user_text, clean)).await;

        self.processing = false;
    }

    async fn handle_command(&mut self, command: &str) {
        let parts: Vec<&str> = command.split_whitespace().collect();
        let cmd = parts.get(0).map(|s| s.to_lowercase()).unwrap_or_default();

        match cmd.as_str() {
            "/quit" | "/exit" => {
                std::process::exit(0);
            }
            "/clear" => {
                self.memory.clear_history().await.ok();
                self.messages.clear();
                self.add_status("🧹", "Chat history cleared");
            }
            "/status" => {
                let jobs = self.scheduler.list_jobs().await.unwrap_or_default();
                let files = self.workspace.list_files();
                self.add_status("🦀", &format!(
                    "Model: {} | Host: {} | Jobs: {} | Files: {}",
                    self.config.ollama.model,
                    self.config.ollama.host,
                    jobs.len(),
                    files.len()
                ));
            }
            "/jobs" => {
                let jobs = self.scheduler.list_jobs().await.unwrap_or_default();
                if jobs.is_empty() {
                    self.add_status("ℹ️", "No scheduled jobs");
                } else {
                    for job in jobs {
                        self.add_status("🕐", &format!("#{}: {} ({})", job.id, job.task, job.schedule));
                    }
                }
            }
            "/workspace" => {
                let files = self.workspace.list_files();
                if files.is_empty() {
                    self.add_status("ℹ️", "Workspace is empty");
                } else {
                    for f in files {
                        let size_kb = f.size as f64 / 1024.0;
                        self.add_status("📁", &format!("{} ({:.1} KB)", f.name, size_kb));
                    }
                }
            }
            "/memory" => {
                let content = self.agent.memory_content().await;
                if content.is_empty() {
                    self.add_status("🧠", "No memories saved yet");
                } else {
                    for line in content.lines().take(10) {
                        self.messages.push((line.to_string(), false));
                    }
                }
            }
            "/forget" => {
                if self.agent.clear_memory().await.is_ok() {
                    self.add_status("🧹", "All memories forgotten");
                } else {
                    self.add_status("❌", "Failed to clear memory");
                }
            }
            "/help" => {
                let help = r#"Commands:
/quit - Exit
/clear - Clear history
/status - Show status
/jobs - List cron jobs
/workspace - List files
/memory - View memories
/forget - Clear memories
/help - This message"#;
                for line in help.lines() {
                    self.messages.push((line.to_string(), false));
                }
            }
            _ => {
                self.process_message(command.to_string()).await;
            }
        }
    }
}

pub async fn run_tui(
    config: Config,
    agent: Arc<Agent>,
    memory: Arc<Memory>,
    scheduler: Arc<Scheduler>,
    workspace: Arc<Workspace>,
) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = TuiApp::new(config, agent, memory, scheduler, workspace);

    app.add_status("🦀", "Welcome to RustyClaw!");
    app.add_status("ℹ️", "Type /help for commands");

    let history = app.memory.get_history(20).await.unwrap_or_default();
    if !history.is_empty() {
        app.messages.push(("── Previous Conversation ──".to_string(), false));
        for msg in history {
            let content = if msg.role == "assistant" {
                Agent::clean_response(&msg.content)
            } else {
                msg.content
            };
            app.add_message(&msg.role, &content);
        }
    }

    loop {
        terminal.draw(|f| ui(f, &app))?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        break;
                    }
                    KeyCode::Enter => {
                        let input = app.input.clone();
                        app.input.clear();
                        
                        if !input.is_empty() {
                            if input.starts_with('/') {
                                app.handle_command(&input).await;
                            } else {
                                app.process_message(input).await;
                            }
                        }
                    }
                    KeyCode::Char(c) => {
                        app.input.push(c);
                    }
                    KeyCode::Backspace => {
                        app.input.pop();
                    }
                    KeyCode::Esc => {
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

fn ui(f: &mut Frame, app: &TuiApp) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(f.area());

    let title = Paragraph::new("🦀 RustyClaw")
        .style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    let messages: Vec<ListItem> = app
        .messages
        .iter()
        .map(|(msg, is_user)| {
            let style = if *is_user {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(Span::styled(msg, style)))
        })
        .collect();

    let messages = List::new(messages)
        .block(Block::default().borders(Borders::ALL).title("Chat"));
    f.render_widget(messages, chunks[1]);

    let input_style = if app.processing {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };
    
    let input = Paragraph::new(app.input.as_str())
        .style(input_style)
        .block(Block::default().borders(Borders::ALL).title(if app.processing { "Thinking..." } else { "Input" }));
    f.render_widget(input, chunks[2]);

    let help = Paragraph::new("Enter: Send | Ctrl+C: Quit | /help for commands")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(help, chunks[3]);
}
