use anyhow::{anyhow, Result};
use chrono::Utc;
use cron::Schedule;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::memory::{CronJob, Memory};

type SendCallback = Arc<dyn Fn(String) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

pub struct Scheduler {
    memory: Memory,
    jobs: Arc<RwLock<HashMap<i64, tokio::task::JoinHandle<()>>>>,
    callbacks: Arc<RwLock<Vec<SendCallback>>>,
}

impl Scheduler {
    pub fn new(memory: Memory) -> Self {
        Self {
            memory,
            jobs: Arc::new(RwLock::new(HashMap::new())),
            callbacks: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn set_send_callback<F, Fut>(&self, callback: F)
    where
        F: Fn(String) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let cb: SendCallback = Arc::new(move |msg| Box::pin(callback(msg)));
        let mut callbacks = self.callbacks.write().await;
        *callbacks = vec![cb];
    }

    pub async fn add_send_callback<F, Fut>(&self, callback: F)
    where
        F: Fn(String) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let cb: SendCallback = Arc::new(move |msg| Box::pin(callback(msg)));
        let mut callbacks = self.callbacks.write().await;
        if !callbacks.iter().any(|c| Arc::ptr_eq(c, &cb)) {
            callbacks.push(cb);
        }
    }

    pub async fn load_jobs(&self) -> Result<()> {
        let jobs = self.memory.get_cron_jobs().await?;
        let count = jobs.len();
        for job in jobs {
            let job_id = job.id;
            if let Err(e) = self.schedule_job(job).await {
                warn!("Failed to load job #{}: {}", job_id, e);
            }
        }
        info!("Loaded {} cron job(s) from database", count);
        Ok(())
    }

    pub async fn add_job(&self, schedule: &str, task: &str, message: &str) -> Result<i64> {
        self.validate_cron(schedule)?;
        
        let job_id = self.memory.add_cron_job(schedule, task, message).await?;
        
        let job = CronJob {
            id: job_id,
            schedule: schedule.to_string(),
            task: task.to_string(),
            message: message.to_string(),
            enabled: true,
        };
        
        self.schedule_job(job).await?;
        info!("Added cron job #{}: '{}' ({})", job_id, task, schedule);
        Ok(job_id)
    }

    pub async fn cancel_job(&self, job_id: i64) -> Result<bool> {
        let success = self.memory.disable_cron_job(job_id).await?;
        
        if success {
            let mut jobs = self.jobs.write().await;
            if let Some(handle) = jobs.remove(&job_id) {
                handle.abort();
            }
            info!("Cancelled cron job #{}", job_id);
        }
        
        Ok(success)
    }

    pub async fn list_jobs(&self) -> Result<Vec<CronJob>> {
        self.memory.get_cron_jobs().await
    }

    fn validate_cron(&self, schedule: &str) -> Result<()> {
        let parts: Vec<&str> = schedule.split_whitespace().collect();
        if parts.len() != 5 {
            return Err(anyhow!(
                "Invalid cron format - needs 5 fields (minute hour day month weekday)"
            ));
        }
        
        Schedule::from_str(schedule)?;
        Ok(())
    }

    async fn schedule_job(&self, job: CronJob) -> Result<()> {
        let schedule = Schedule::from_str(&job.schedule)?;
        let callbacks = self.callbacks.clone();
        let message = job.message.clone();
        let job_id = job.id;
        let jobs = self.jobs.clone();

        let handle = tokio::spawn(async move {
            loop {
                let next = schedule.upcoming(Utc).next();
                if let Some(next_time) = next {
                    let now = Utc::now();
                    let delay = next_time - now;
                    
                    if delay.num_seconds() > 0 {
                        tokio::time::sleep(Duration::from_secs(delay.num_seconds() as u64)).await;
                    }

                    info!("Cron job #{} triggered: {}", job_id, message);
                    
                    let cbs = callbacks.read().await;
                    if cbs.is_empty() {
                        warn!("No send callbacks registered — cron message dropped");
                    } else {
                        for callback in cbs.iter() {
                            callback(message.clone()).await;
                        }
                    }
                } else {
                    break;
                }
            }
            
            let mut j = jobs.write().await;
            j.remove(&job_id);
        });

        let mut jobs = self.jobs.write().await;
        jobs.insert(job.id, handle);

        Ok(())
    }

    pub fn stop(&self) {
        // Abort all running jobs
        if let Ok(jobs) = self.jobs.try_write() {
            for (_, handle) in jobs.iter() {
                handle.abort();
            }
        }
    }
}
