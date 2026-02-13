use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramConfig {
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub allowed_users: Vec<i64>,
}

impl Default for TelegramConfig {
    fn default() -> Self {
        Self {
            token: String::new(),
            allowed_users: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OllamaConfig {
    #[serde(default = "default_ollama_host")]
    pub host: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_keep_alive")]
    pub keep_alive: i64,
    #[serde(default = "default_context_length")]
    pub context_length: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
}

fn default_ollama_host() -> String {
    "http://localhost:11434".to_string()
}

fn default_model() -> String {
    "tinyllama".to_string()
}

fn default_keep_alive() -> i64 {
    -1
}

fn default_context_length() -> u32 {
    4096
}

fn default_temperature() -> f32 {
    0.7
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            host: default_ollama_host(),
            model: default_model(),
            keep_alive: default_keep_alive(),
            context_length: default_context_length(),
            temperature: default_temperature(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceConfig {
    #[serde(default = "default_workspace_path")]
    pub path: PathBuf,
}

fn default_workspace_path() -> PathBuf {
    PathBuf::from("./workspace")
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            path: default_workspace_path(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SchedulerConfig {
    #[serde(default = "default_scheduler_enabled")]
    pub enabled: bool,
}

fn default_scheduler_enabled() -> bool {
    true
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            enabled: default_scheduler_enabled(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct MemoryConfig {
    #[serde(default = "default_database_path")]
    pub database: PathBuf,
    #[serde(default = "default_max_history")]
    pub max_history: usize,
}

fn default_database_path() -> PathBuf {
    PathBuf::from("./rustyclaw.db")
}

fn default_max_history() -> usize {
    50
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            database: default_database_path(),
            max_history: default_max_history(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub telegram: TelegramConfig,
    #[serde(default)]
    pub ollama: OllamaConfig,
    #[serde(default)]
    pub workspace: WorkspaceConfig,
    #[serde(default)]
    pub scheduler: SchedulerConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub system_prompt: String,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            anyhow::bail!(
                "Config file not found: {}\nCopy config.example.yaml to config.yaml and edit it.",
                path.display()
            );
        }

        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let mut config: Config =
            serde_yaml::from_str(&content).with_context(|| "Failed to parse config YAML")?;

        if config.system_prompt.is_empty() {
            let soul_path = Path::new("soul.md");
            if soul_path.exists() {
                config.system_prompt =
                    std::fs::read_to_string(soul_path).with_context(|| "Failed to read soul.md")?;
            }
        }

        Ok(config)
    }

    pub fn load_from_default() -> Result<Self> {
        Self::load(Path::new("config.yaml"))
    }
}
