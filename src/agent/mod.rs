use anyhow::{anyhow, Result};
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::config::OllamaConfig;
use crate::memory::Message;

const MEMORY_FILE: &str = "memory.md";
const MAX_MEMORY_LINES: usize = 100;

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    message: ChatMessage,
}

#[derive(Debug, Clone)]
pub struct CronJobData {
    pub schedule: String,
    pub task: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct SaveBlock {
    pub filename: String,
    pub content: String,
}

pub struct Agent {
    config: OllamaConfig,
    base_prompt: String,
    memory_content: Arc<RwLock<String>>,
    system_prompt: Arc<RwLock<String>>,
    client: Client,
    memory_path: PathBuf,
}

impl Agent {
    pub fn new(config: OllamaConfig, system_prompt: String) -> Self {
        let memory_content = Self::load_memory(Path::new(MEMORY_FILE));
        let full_prompt = Self::build_full_prompt(&system_prompt, &memory_content);
        
        Self {
            config,
            base_prompt: system_prompt,
            memory_content: Arc::new(RwLock::new(memory_content)),
            system_prompt: Arc::new(RwLock::new(full_prompt)),
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap(),
            memory_path: PathBuf::from(MEMORY_FILE),
        }
    }

    fn load_memory(path: &Path) -> String {
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(content) if !content.trim().is_empty() => {
                    info!("Loaded {} lines from memory.md", content.lines().count());
                    return content;
                }
                Ok(_) => {}
                Err(e) => warn!("Failed to load memory.md: {}", e),
            }
        }
        String::new()
    }

    fn build_full_prompt(base: &str, memory: &str) -> String {
        if memory.is_empty() {
            base.to_string()
        } else {
            format!(
                "{}\n\n## Personal Memory\nThese are important facts to remember about the user:\n{}",
                base, memory
            )
        }
    }

    pub async fn check_memory_size(&self) -> (bool, usize) {
        let content = self.memory_content.read().await;
        let lines = if content.is_empty() { 0 } else { content.lines().count() };
        (lines > MAX_MEMORY_LINES, lines)
    }

    pub async fn save_to_memory(&self, fact: &str) -> Result<bool> {
        let memory = self.memory_content.read().await;
        if memory.contains(fact.trim()) {
            debug!("Fact already in memory: {}", fact);
            return Ok(false);
        }
        drop(memory);

        let fact_line = format!("- {}\n", fact.trim());
        
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.memory_path)?;
        
        use std::io::Write;
        if self.memory_path.exists() && self.memory_path.metadata()?.len() > 0 {
            write!(file, "\n{}", fact_line)?;
        } else {
            write!(file, "{}", fact_line)?;
        }

        let new_memory = Self::load_memory(&self.memory_path);
        let new_prompt = Self::build_full_prompt(&self.base_prompt, &new_memory);
        
        let mut memory = self.memory_content.write().await;
        *memory = new_memory;
        drop(memory);
        
        let mut prompt = self.system_prompt.write().await;
        *prompt = new_prompt;

        info!("Saved to memory: {}", fact);
        Ok(true)
    }

    pub async fn clear_memory(&self) -> Result<bool> {
        if self.memory_path.exists() {
            std::fs::remove_file(&self.memory_path)?;
        }
        
        {
            let mut memory = self.memory_content.write().await;
            *memory = String::new();
        }
        
        let mut prompt = self.system_prompt.write().await;
        *prompt = self.base_prompt.clone();
        
        info!("Memory cleared");
        Ok(true)
    }

    pub async fn memory_content(&self) -> String {
        self.memory_content.read().await.clone()
    }

    pub async fn warm_up(&self) -> Result<()> {
        info!("Warming up model: {}", self.config.model);
        
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: "hi".to_string(),
        }];

        match self.chat_request(&messages).await {
            Ok(_) => info!("Model loaded and ready"),
            Err(e) => warn!("Warm-up failed, continuing anyway: {}", e),
        }

        Ok(())
    }

    async fn chat_request(&self, messages: &[ChatMessage]) -> Result<String> {
        let url = format!("{}/api/chat", self.config.host);
        
        let system_prompt = self.system_prompt.read().await.clone();
        let mut full_messages = vec![ChatMessage {
            role: "system".to_string(),
            content: system_prompt,
        }];
        full_messages.extend(messages.iter().cloned());

        let request = ChatRequest {
            model: self.config.model.clone(),
            messages: full_messages,
            stream: Some(false),
        };

        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(anyhow!("Ollama returned error {}: {}", status, text));
        }

        let data: ChatResponse = response.json().await?;
        Ok(data.message.content)
    }

    pub async fn chat(&self, messages: &[Message]) -> Result<String> {
        let chat_messages: Vec<ChatMessage> = messages
            .iter()
            .map(|m| ChatMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();

        match self.chat_request(&chat_messages).await {
            Ok(response) => Ok(response),
            Err(e) => {
                warn!("Ollama chat error: {}", e);
                Ok(format!("Sorry, I had trouble thinking about that. Error: {}", e))
            }
        }
    }

    pub fn parse_cron_blocks(text: &str) -> (Vec<CronJobData>, Vec<String>) {
        let re = Regex::new(r"```cron\s*\n(.*?)\n\s*```").unwrap();
        let mut jobs = Vec::new();
        let mut errors = Vec::new();

        for cap in re.captures_iter(text) {
            let json_str = cap[1].trim();
            match serde_json::from_str::<serde_json::Value>(json_str) {
                Ok(json) => {
                    let missing: Vec<&str> = ["schedule", "task", "message"]
                        .iter()
                        .filter(|k| !json.get(**k).is_some())
                        .copied()
                        .collect();

                    if !missing.is_empty() {
                        errors.push(format!("Missing required fields: {}", missing.join(", ")));
                        continue;
                    }

                    let schedule = json["schedule"].as_str().unwrap_or("").to_string();
                    let parts: Vec<&str> = schedule.split_whitespace().collect();
                    
                    if parts.len() != 5 {
                        errors.push(format!(
                            "Invalid cron format '{}' - needs 5 fields (minute hour day month weekday)",
                            schedule
                        ));
                        continue;
                    }

                    jobs.push(CronJobData {
                        schedule,
                        task: json["task"].as_str().unwrap_or("").to_string(),
                        message: json["message"].as_str().unwrap_or("").to_string(),
                    });
                }
                Err(_) => {
                    errors.push("Invalid JSON in cron block".to_string());
                }
            }
        }

        (jobs, errors)
    }

    pub fn parse_save_blocks(text: &str) -> Vec<SaveBlock> {
        let re = Regex::new(r"```save:(\S+)\s*\n(.*?)\n\s*```").unwrap();
        re.captures_iter(text)
            .map(|cap| SaveBlock {
                filename: cap[1].to_string(),
                content: cap[2].to_string(),
            })
            .collect()
    }

    pub fn parse_memory_blocks(text: &str) -> Vec<String> {
        let re = Regex::new(r"```memory\s*\n(.*?)\n\s*```").unwrap();
        re.captures_iter(text)
            .map(|cap| cap[1].trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    pub fn extract_code_blocks(text: &str) -> Vec<(String, String)> {
        let re = Regex::new(r"```(\w+)?\s*\n(.*?)\n\s*```").unwrap();
        re.captures_iter(text)
            .map(|cap| {
                let lang = cap.get(1).map(|m| m.as_str()).unwrap_or("text");
                (lang.to_string(), cap[2].trim().to_string())
            })
            .collect()
    }

    pub fn clean_response(text: &str) -> String {
        let mut result = text.to_string();
        
        let re_cron = Regex::new(r"```cron\s*\n.*?\n\s*```").unwrap();
        result = re_cron.replace_all(&result, "").to_string();
        
        let re_save = Regex::new(r"```save:\S+\s*\n.*?\n\s*```").unwrap();
        result = re_save.replace_all(&result, "").to_string();
        
        let re_memory = Regex::new(r"```memory\s*\n.*?\n\s*```").unwrap();
        result = re_memory.replace_all(&result, "").to_string();
        
        result.trim().to_string()
    }
}
