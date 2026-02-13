use anyhow::Result;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use sqlx::Row;
use std::path::Path;

const USER_ID: i64 = 1;

#[derive(Debug, Clone)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct CronJob {
    pub id: i64,
    pub schedule: String,
    pub task: String,
    pub message: String,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct WorkspaceFile {
    pub filename: String,
    pub description: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct Memory {
    pool: SqlitePool,
}

impl Memory {
    pub async fn connect(db_path: &Path) -> Result<Self> {
        let db_url = format!("sqlite:{}?mode=rwc", db_path.display());
        
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&db_url)
            .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS conversations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id INTEGER NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS cron_jobs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id INTEGER NOT NULL,
                schedule TEXT NOT NULL,
                task TEXT NOT NULL,
                message TEXT NOT NULL,
                enabled INTEGER DEFAULT 1,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS workspace_files (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                filename TEXT NOT NULL,
                description TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            "#,
        )
        .execute(&pool)
        .await?;

        Ok(Self { pool })
    }

    pub async fn add_message(&self, role: &str, content: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO conversations (user_id, role, content) VALUES (?, ?, ?)",
        )
        .bind(USER_ID)
        .bind(role)
        .bind(content)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_history(&self, limit: usize) -> Result<Vec<Message>> {
        let rows = sqlx::query(
            "SELECT role, content FROM conversations \
             WHERE user_id = ? ORDER BY id DESC LIMIT ?",
        )
        .bind(USER_ID)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut messages: Vec<Message> = rows
            .into_iter()
            .map(|row| Message {
                role: row.get("role"),
                content: row.get("content"),
            })
            .collect();

        messages.reverse();
        Ok(messages)
    }

    pub async fn clear_history(&self) -> Result<()> {
        sqlx::query("DELETE FROM conversations WHERE user_id = ?")
            .bind(USER_ID)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn add_cron_job(&self, schedule: &str, task: &str, message: &str) -> Result<i64> {
        let result = sqlx::query(
            "INSERT INTO cron_jobs (user_id, schedule, task, message) VALUES (?, ?, ?, ?)",
        )
        .bind(USER_ID)
        .bind(schedule)
        .bind(task)
        .bind(message)
        .execute(&self.pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    pub async fn get_cron_jobs(&self) -> Result<Vec<CronJob>> {
        let rows = sqlx::query(
            "SELECT id, user_id, schedule, task, message, enabled \
             FROM cron_jobs WHERE user_id = ? AND enabled = 1",
        )
        .bind(USER_ID)
        .fetch_all(&self.pool)
        .await?;

        let jobs = rows
            .into_iter()
            .map(|row| CronJob {
                id: row.get("id"),
                schedule: row.get("schedule"),
                task: row.get("task"),
                message: row.get("message"),
                enabled: row.get::<i64, _>("enabled") == 1,
            })
            .collect();

        Ok(jobs)
    }

    pub async fn disable_cron_job(&self, job_id: i64) -> Result<bool> {
        let result = sqlx::query("UPDATE cron_jobs SET enabled = 0 WHERE id = ?")
            .bind(job_id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn log_file(&self, filename: &str, description: Option<&str>) -> Result<()> {
        sqlx::query(
            "INSERT INTO workspace_files (filename, description) VALUES (?, ?)",
        )
        .bind(filename)
        .bind(description)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_workspace_files(&self) -> Result<Vec<WorkspaceFile>> {
        let rows = sqlx::query(
            "SELECT filename, description, created_at FROM workspace_files ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        let files = rows
            .into_iter()
            .map(|row| WorkspaceFile {
                filename: row.get("filename"),
                description: row.get("description"),
                created_at: row.get("created_at"),
            })
            .collect();

        Ok(files)
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}
