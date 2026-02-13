use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tracing::info;

use crate::memory::Memory;

#[derive(Debug, Clone)]
pub struct FileInfo {
    pub name: String,
    pub size: u64,
    pub modified: SystemTime,
}

pub struct Workspace {
    path: PathBuf,
    memory: Memory,
}

impl Workspace {
    pub fn new(path: PathBuf, memory: Memory) -> Result<Self> {
        std::fs::create_dir_all(&path)?;
        Ok(Self { path, memory })
    }

    pub async fn save_file(&self, filename: &str, content: &str) -> Result<PathBuf> {
        let safe_name = Path::new(filename)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("untitled.txt");
        
        let filepath = self.path.join(safe_name);
        
        let final_path = if filepath.exists() {
            let stem = filepath.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("untitled")
                .to_string();
            let suffix = filepath.extension()
                .and_then(|s| s.to_str())
                .unwrap_or("txt")
                .to_string();
            
            let mut counter = 1;
            loop {
                let new_path = self.path.join(format!("{}_{}.{}", stem, counter, suffix));
                if !new_path.exists() {
                    break new_path;
                }
                counter += 1;
            }
        } else {
            filepath
        };

        std::fs::write(&final_path, content)?;
        
        let final_name = final_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(safe_name);
        
        self.memory.log_file(final_name, Some(&format!("Generated file: {}", safe_name))).await?;
        
        info!("Saved file: {:?}", final_path);
        Ok(final_path)
    }

    pub fn list_files(&self) -> Vec<FileInfo> {
        let mut files = Vec::new();
        
        if let Ok(entries) = std::fs::read_dir(&self.path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Ok(metadata) = entry.metadata() {
                        let name = path.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown")
                            .to_string();
                        
                        files.push(FileInfo {
                            name,
                            size: metadata.len(),
                            modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                        });
                    }
                }
            }
        }
        
        files.sort_by(|a, b| a.name.cmp(&b.name));
        files
    }

    pub fn read_file(&self, filename: &str) -> Option<String> {
        let safe_name = Path::new(filename)
            .file_name()
            .and_then(|n| n.to_str())?;
        
        let filepath = self.path.join(safe_name);
        
        if filepath.exists() && filepath.is_file() {
            std::fs::read_to_string(filepath).ok()
        } else {
            None
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
