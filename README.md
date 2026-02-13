# 🦀 RustyClaw

A lightweight, local AI assistant — Rust port of [PiLobster](https://github.com/kevinmcaleer/pilobster).

RustyClaw connects a local Ollama model to Telegram, lets you chat with your AI via terminal, schedule cron jobs, and generate code — all running on your own hardware with zero cloud dependencies.

## Features

- **Telegram Chat** — Talk to your local LLM from anywhere via Telegram
- **Terminal UI (TUI)** — Chat interface directly in your terminal using ratatui
- **Cron Scheduler** — Create recurring tasks via natural conversation
- **Code Workspace** — Ask it to generate code and it saves files locally
- **Persistent Memory** — Conversation history and task memory stored in SQLite
- **Keep-Alive** — Model stays loaded in memory (no cold-start delays)
- **Multi-Mode** — Run Telegram bot, TUI, or both simultaneously

## Requirements

- Rust 1.70+
- Ollama installed and running
- A model pulled in Ollama (e.g. `ollama pull tinyllama`)
- A Telegram Bot Token (from @BotFather) — Optional: only for Telegram mode

## Quick Start

```bash
# Clone and build
cd rustyclaw
cargo build --release

# Copy the example config and edit it
cp config.example.yaml config.yaml
nano config.yaml  # Add your Telegram bot token and model name

# Run it (Both Telegram and TUI - default)
./target/release/rustyclaw

# Or run in Terminal UI mode only (no Telegram needed!)
./target/release/rustyclaw --mode tui

# Or run in Telegram mode only
./target/release/rustyclaw --mode telegram
```

## Configuration

Edit `config.yaml`:

```yaml
telegram:
  token: "YOUR_BOT_TOKEN_HERE"

ollama:
  host: "http://localhost:11434"
  model: "tinyllama"
  keep_alive: -1        # Keep model loaded forever
  context_length: 4096

workspace:
  path: "./workspace"

scheduler:
  enabled: true

memory:
  database: "./rustyclaw.db"
  max_history: 50
```

## Customizing Personality

Edit `soul.md` to customize your bot's personality and instructions.

## Architecture

```
┌─────────────┐     ┌──────────────┐     ┌─────────────┐
│  Telegram   │◄───►│  RustyClaw   │◄───►│   Ollama    │
│  (mobile)   │     │    (Rust)    │     │ (tinyllama) │
└─────────────┘     └──────┬───────┘     └─────────────┘
                           │
                    ┌──────┴───────┐
                    │   SQLite     │
                    │  (memory +   │
                    │   cron jobs) │
                    └──────────────┘
```

## Commands

In Telegram:
- `/start` — Welcome message
- `/status` — Show system status
- `/jobs` — List scheduled cron jobs
- `/schedule <cron> <msg>` — Create a cron job
- `/cancel <id>` — Cancel a scheduled job
- `/workspace` — List files in workspace
- `/save <filename>` — Save last code block
- `/memory` — View saved memories
- `/forget` — Clear all memories
- `/clear` — Clear chat history
- `/help` — Show available commands

## Comparison with PiLobster

| Feature | PiLobster (Python) | RustyClaw (Rust) |
|---------|-------------------|------------------|
| Runtime | asyncio | tokio |
| Telegram | python-telegram-bot | teloxide |
| TUI | Textual | ratatui |
| Database | aiosqlite | sqlx |
| Scheduler | APScheduler | cron crate |
| Memory Safety | GC | Borrow checker |
| Binary Size | ~50MB+ (Python + deps) | ~10MB (release) |
| Startup Time | ~1-2s | <100ms |

## License

MIT
