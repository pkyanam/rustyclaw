use anyhow::Result;
use std::sync::Arc;
use teloxide::{
    prelude::*,
    types::{BotCommand, ChatId},
    utils::command::BotCommands,
};
use tokio::sync::RwLock;
use tracing::info;

use crate::agent::Agent;
use crate::config::Config;
use crate::memory::Memory;
use crate::scheduler::Scheduler;
use crate::workspace::Workspace;

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase")]
enum Command {
    #[command(description = "Welcome message")]
    Start,
    #[command(description = "Show system status")]
    Status,
    #[command(description = "List scheduled tasks")]
    Jobs,
    #[command(description = "Create a cron job")]
    Schedule,
    #[command(description = "Cancel a scheduled task")]
    Cancel,
    #[command(description = "List generated files")]
    Workspace,
    #[command(description = "Save last code block")]
    Save,
    #[command(description = "View saved memories")]
    Memory,
    #[command(description = "Clear all memories")]
    Forget,
    #[command(description = "Clear chat history")]
    Clear,
    #[command(description = "Show commands")]
    Help,
}

pub struct TelegramBot {
    config: Config,
    agent: Arc<Agent>,
    memory: Arc<Memory>,
    scheduler: Arc<Scheduler>,
    workspace: Arc<Workspace>,
    chat_id: Arc<RwLock<Option<ChatId>>>,
    tui_callback: Arc<RwLock<Option<Box<dyn Fn(String, bool) + Send + Sync>>>>,
}

impl TelegramBot {
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
            chat_id: Arc::new(RwLock::new(None)),
            tui_callback: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn set_tui_callback<F>(&self, callback: F)
    where
        F: Fn(String, bool) + Send + Sync + 'static,
    {
        let mut cb = self.tui_callback.write().await;
        *cb = Some(Box::new(callback));
    }

    async fn send_to_tui(&self, message: &str, is_user: bool) {
        let cb = self.tui_callback.read().await;
        if let Some(callback) = cb.as_ref() {
            callback(message.to_string(), is_user);
        }
    }

    async fn send_to_telegram(&self, bot: &Bot, message: &str) {
        let chat_id = self.chat_id.read().await;
        if let Some(chat_id) = *chat_id {
            for chunk in message.as_bytes().chunks(4000) {
                let text = String::from_utf8_lossy(chunk).to_string();
                if let Err(e) = bot.send_message(chat_id, &text).await {
                    tracing::error!("Failed to send message to Telegram: {}", e);
                }
            }
        }
    }

    pub async fn run(&self) -> Result<()> {
        let bot = Bot::new(self.config.telegram.token.clone());
        
        bot.set_my_commands(vec![
            BotCommand::new("start", "Welcome message"),
            BotCommand::new("status", "Show system status"),
            BotCommand::new("jobs", "List scheduled tasks"),
            BotCommand::new("schedule", "Create a cron job"),
            BotCommand::new("cancel", "Cancel a scheduled task"),
            BotCommand::new("workspace", "List generated files"),
            BotCommand::new("save", "Save last code block"),
            BotCommand::new("memory", "View saved memories"),
            BotCommand::new("forget", "Clear all memories"),
            BotCommand::new("clear", "Clear chat history"),
            BotCommand::new("help", "Show commands"),
        ]).await?;

        let agent = self.agent.clone();
        let memory = self.memory.clone();
        let scheduler = self.scheduler.clone();
        let workspace = self.workspace.clone();
        let config = self.config.clone();
        let chat_id = self.chat_id.clone();
        let tui_callback = self.tui_callback.clone();

        info!("🦀 Telegram bot is ready! Waiting for messages...");

        let handler = Update::filter_message()
            .branch(dptree::entry().filter_command::<Command>().endpoint(handle_command))
            .branch(dptree::endpoint(handle_message));

        Dispatcher::builder(bot.clone(), handler)
            .dependencies(dptree::deps![
                agent,
                memory,
                scheduler,
                workspace,
                Arc::new(config),
                chat_id,
                tui_callback
            ])
            .enable_ctrlc_handler()
            .build()
            .dispatch()
            .await;

        Ok(())
    }
}

async fn handle_command(
    bot: Bot,
    msg: Message,
    cmd: Command,
    agent: Arc<Agent>,
    memory: Arc<Memory>,
    scheduler: Arc<Scheduler>,
    workspace: Arc<Workspace>,
    config: Arc<Config>,
) -> ResponseResult<()> {
    let chat_id = msg.chat.id;
    
    let response = match cmd {
        Command::Start => {
            "🦀 RustyClaw is online!\n\n\
            I'm your local AI assistant running in Rust.\n\n\
            Just send me a message to chat, or use:\n\
            /status — System status\n\
            /jobs — List scheduled tasks\n\
            /schedule — Create a cron job\n\
            /workspace — List generated files\n\
            /clear — Clear conversation history\n\
            /help — Show all commands".to_string()
        }
        Command::Status => {
            let jobs = scheduler.list_jobs().await.unwrap_or_default();
            let files = workspace.list_files();
            format!(
                "🦀 RustyClaw Status\n\n\
                Model: {}\n\
                Host: {}\n\
                Context: {} tokens\n\
                Scheduled jobs: {}\n\
                Workspace files: {}",
                config.ollama.model,
                config.ollama.host,
                config.ollama.context_length,
                jobs.len(),
                files.len()
            )
        }
        Command::Jobs => {
            let jobs = scheduler.list_jobs().await.unwrap_or_default();
            if jobs.is_empty() {
                "No scheduled jobs. Ask me to schedule something!".to_string()
            } else {
                let mut lines = vec!["🕐 Scheduled Jobs\n".to_string()];
                for job in jobs {
                    lines.push(format!("#{} — {}\n  Schedule: {}", job.id, job.task, job.schedule));
                }
                lines.join("\n")
            }
        }
        Command::Cancel => {
            "Usage: /cancel <job_id>".to_string()
        }
        Command::Workspace => {
            let files = workspace.list_files();
            if files.is_empty() {
                "Workspace is empty. Ask me to write some code!".to_string()
            } else {
                let mut lines = vec!["📁 Workspace Files\n".to_string()];
                for f in files {
                    let size_kb = f.size as f64 / 1024.0;
                    lines.push(format!("{} ({:.1} KB)", f.name, size_kb));
                }
                lines.join("\n")
            }
        }
        Command::Clear => {
            memory.clear_history().await.ok();
            "🧹 Conversation history cleared.".to_string()
        }
        Command::Memory => {
            let memory_content = agent.memory_content().await;
            let (is_large, line_count) = agent.check_memory_size().await;
            if memory_content.is_empty() {
                "🧠 My Memory\n\nNo memories saved yet. Tell me something about yourself!".to_string()
            } else {
                let header = if is_large {
                    format!("🧠 My Memory ({} lines)\n\n⚠️ Memory is getting large!\n\n", line_count)
                } else {
                    format!("🧠 My Memory ({} lines)\n\n", line_count)
                };
                format!("{}{}", header, memory_content)
            }
        }
        Command::Forget => {
            if agent.clear_memory().await.is_ok() {
                "🧹 All memories have been forgotten.".to_string()
            } else {
                "❌ Failed to clear memory.".to_string()
            }
        }
        Command::Save => {
            "Usage: /save filename.py\n\nThis will save the last code block from my response.".to_string()
        }
        Command::Schedule => {
            "Usage: /schedule <cron> <prompt>\n\n\
            The prompt will be sent to me when the job triggers.\n\n\
            Cron format: minute hour day month weekday\n\n\
            Examples:\n\
            /schedule */3 * * * * Tell me a joke\n\
            /schedule 0 9 * * * Give me a motivational quote".to_string()
        }
        Command::Help => {
            "🦀 RustyClaw Commands\n\n\
            /start — Welcome message\n\
            /status — System status\n\
            /jobs — List scheduled tasks\n\
            /schedule <cron> <msg> — Create a cron job\n\
            /cancel <id> — Cancel a task\n\
            /workspace — List generated files\n\
            /save <filename> — Save last code block\n\
            /memory — View saved memories\n\
            /forget — Clear all memories\n\
            /clear — Clear chat history\n\
            /help — This message".to_string()
        }
    };

    for chunk in response.as_bytes().chunks(4000) {
        let text = String::from_utf8_lossy(chunk).to_string();
        bot.send_message(chat_id, &text).await?;
    }

    Ok(())
}

async fn handle_message(
    bot: Bot,
    msg: Message,
    agent: Arc<Agent>,
    memory: Arc<Memory>,
    scheduler: Arc<Scheduler>,
    workspace: Arc<Workspace>,
    config: Arc<Config>,
    chat_id_storage: Arc<RwLock<Option<ChatId>>>,
) -> ResponseResult<()> {
    let chat_id = msg.chat.id;
    
    {
        let mut stored = chat_id_storage.write().await;
        *stored = Some(chat_id);
    }

    let user_text = match msg.text() {
        Some(text) => text.to_string(),
        None => return Ok(()),
    };

    if user_text.starts_with("/cancel ") {
        let parts: Vec<&str> = user_text.split_whitespace().collect();
        if parts.len() >= 2 {
            if let Ok(job_id) = parts[1].parse::<i64>() {
                match scheduler.cancel_job(job_id).await {
                    Ok(true) => {
                        bot.send_message(chat_id, format!("✅ Cancelled job #{}", job_id)).await?;
                    }
                    Ok(false) => {
                        bot.send_message(chat_id, format!("Job #{} not found.", job_id)).await?;
                    }
                    Err(e) => {
                        bot.send_message(chat_id, format!("Error: {}", e)).await?;
                    }
                }
            }
        }
        return Ok(());
    }

    if user_text.starts_with("/schedule ") {
        let parts: Vec<&str> = user_text.split_whitespace().collect();
        if parts.len() >= 7 {
            let schedule = parts[1..6].join(" ");
            let message = parts[6..].join(" ");
            let task = if message.len() > 50 {
                format!("{}...", &message[..47])
            } else {
                message.clone()
            };
            
            match scheduler.add_job(&schedule, &task, &message).await {
                Ok(job_id) => {
                    let response = format!(
                        "✅ Scheduled job #{}: {}\nSchedule: {}\nMessage: {}",
                        job_id, task, schedule, message
                    );
                    bot.send_message(chat_id, &response).await?;
                }
                Err(e) => {
                    let error = format!("❌ Invalid cron expression: {}", e);
                    bot.send_message(chat_id, &error).await?;
                }
            }
        } else {
            bot.send_message(chat_id, "Usage: /schedule <cron> <message>").await?;
        }
        return Ok(());
    }

    if user_text.starts_with("/save ") {
        let parts: Vec<&str> = user_text.split_whitespace().collect();
        if parts.len() >= 2 {
            let filename = parts[1];
            
            if let Ok(history) = memory.get_history(10).await {
                for msg in history.iter().rev() {
                    if msg.role == "assistant" {
                        let code_blocks = Agent::extract_code_blocks(&msg.content);
                        if !code_blocks.is_empty() {
                            match workspace.save_file(filename, &code_blocks[0].1).await {
                                Ok(path) => {
                                    let name = path.file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or(filename);
                                    bot.send_message(chat_id, format!("💾 Saved {} to workspace", name)).await?;
                                }
                                Err(e) => {
                                    bot.send_message(chat_id, format!("❌ Error saving file: {}", e)).await?;
                                }
                            }
                            return Ok(());
                        }
                    }
                }
            }
            bot.send_message(chat_id, "❌ No code blocks found in recent conversation.").await?;
        }
        return Ok(());
    }

    info!("Message received: {}...", &user_text[..user_text.len().min(80)]);

    memory.add_message("user", &user_text).await.ok();

    let history = memory.get_history(config.memory.max_history).await.unwrap_or_default();

    bot.send_chat_action(chat_id, teloxide::types::ChatAction::Typing).await?;

    let response = agent.chat(&history).await.unwrap_or_else(|e| {
        format!("Sorry, I had trouble thinking about that. Error: {}", e)
    });

    let (cron_jobs, cron_errors) = Agent::parse_cron_blocks(&response);
    
    for error in cron_errors {
        bot.send_message(chat_id, format!("⚠️ Cron error: {}", error)).await?;
    }

    for job in cron_jobs {
        match scheduler.add_job(&job.schedule, &job.task, &job.message).await {
            Ok(job_id) => {
                let msg = format!(
                    "✅ Scheduled job #{}: {}\nSchedule: {}",
                    job_id, job.task, job.schedule
                );
                bot.send_message(chat_id, &msg).await?;
            }
            Err(e) => {
                bot.send_message(chat_id, format!("❌ Error scheduling: {}", e)).await?;
            }
        }
    }

    let save_blocks = Agent::parse_save_blocks(&response);
    for block in save_blocks {
        match workspace.save_file(&block.filename, &block.content).await {
            Ok(path) => {
                let name = path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&block.filename);
                bot.send_message(chat_id, format!("💾 Saved {} to workspace", name)).await?;
            }
            Err(e) => {
                bot.send_message(chat_id, format!("❌ Error saving file: {}", e)).await?;
            }
        }
    }

    let memory_blocks = Agent::parse_memory_blocks(&response);
    for fact in memory_blocks {
        if agent.save_to_memory(&fact).await.unwrap_or(false) {
            bot.send_message(chat_id, format!("🧠 Remembered: {}", fact)).await?;
        }
    }

    let clean = Agent::clean_response(&response);
    if !clean.is_empty() {
        for chunk in clean.as_bytes().chunks(4000) {
            let text = String::from_utf8_lossy(chunk).to_string();
            bot.send_message(chat_id, &text).await?;
        }
    }

    memory.add_message("assistant", &response).await.ok();

    Ok(())
}
