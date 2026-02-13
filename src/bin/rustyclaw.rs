use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

use rustyclaw::{
    agent::Agent,
    config::Config,
    memory::Memory,
    scheduler::Scheduler,
    telegram::TelegramBot,
    tui::run_tui,
    workspace::Workspace,
    VERSION,
};

#[derive(Parser, Debug)]
#[command(name = "rustyclaw")]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "config.yaml")]
    config: PathBuf,

    #[arg(short, long, value_enum, default_value = "both")]
    mode: Mode,
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum Mode {
    Telegram,
    Tui,
    Both,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    if matches!(args.mode, Mode::Tui | Mode::Both) {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_writer(|| std::fs::File::create("rustyclaw.log").unwrap())
            .init();
    } else {
        tracing_subscriber::fmt::init();
    }

    println!("🦀 RustyClaw v{} | Mode: {:?}", VERSION, args.mode);
    
    if matches!(args.mode, Mode::Tui | Mode::Both) {
        println!("Logs: rustyclaw.log");
    }
    println!();

    let config = Config::load(&args.config)?;

    if matches!(args.mode, Mode::Telegram | Mode::Both) {
        if config.telegram.token.is_empty() || config.telegram.token == "YOUR_BOT_TOKEN_HERE" {
            eprintln!("Error: Telegram mode requires a valid bot token in config.yaml");
            eprintln!("Set your token or use --mode tui to skip Telegram");
            std::process::exit(1);
        }
    }

    let memory = Arc::new(Memory::connect(&config.memory.database).await?);
    info!("Database connected: {:?}", config.memory.database);

    let agent = Arc::new(Agent::new(config.ollama.clone(), config.system_prompt.clone()));
    agent.warm_up().await?;

    let workspace = Arc::new(Workspace::new(config.workspace.path.clone(), memory.as_ref().clone())?);
    info!("Workspace: {:?}", workspace.path());

    let scheduler = Arc::new(Scheduler::new(memory.as_ref().clone()));
    
    if config.scheduler.enabled {
        scheduler.load_jobs().await?;
    }

    match args.mode {
        Mode::Telegram => {
            let bot = TelegramBot::new(
                config.clone(),
                agent,
                memory.clone(),
                scheduler.clone(),
                workspace,
            );
            
            scheduler.set_send_callback(|msg: String| {
                async move {
                    info!("Cron message: {}", msg);
                }
            }).await;

            bot.run().await?;
        }
        Mode::Tui => {
            run_tui(config.clone(), agent, memory.clone(), scheduler.clone(), workspace).await?;
        }
        Mode::Both => {
            let bot = Arc::new(TelegramBot::new(
                config.clone(),
                agent.clone(),
                memory.clone(),
                scheduler.clone(),
                workspace.clone(),
            ));

            let bot_clone = bot.clone();
            let agent_clone = agent.clone();
            let memory_clone = memory.clone();

            scheduler.set_send_callback(move |msg: String| {
                let agent = agent_clone.clone();
                let memory = memory_clone.clone();
                async move {
                    info!("Cron message: {}", msg);
                    memory.add_message("user", &msg).await.ok();
                    if let Ok(history) = memory.get_history(50).await {
                        if let Ok(response) = agent.chat(&history).await {
                            let clean = Agent::clean_response(&response);
                            info!("Cron response: {}", clean);
                        }
                    }
                }
            }).await;

            let telegram_handle = tokio::spawn(async move {
                if let Err(e) = bot_clone.run().await {
                    eprintln!("Telegram error: {}", e);
                }
            });

            let tui_memory = memory.clone();
            let tui_scheduler = scheduler.clone();
            let tui_handle = tokio::spawn(async move {
                if let Err(e) = run_tui(config.clone(), agent, tui_memory, tui_scheduler, workspace).await {
                    eprintln!("TUI error: {}", e);
                }
            });

            tokio::select! {
                _ = telegram_handle => {}
                _ = tui_handle => {}
            }
        }
    }

    scheduler.stop();
    memory.close().await;
    info!("Goodbye! 🦀");

    Ok(())
}
