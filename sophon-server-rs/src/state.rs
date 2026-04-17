use crate::models::TaskStatus;
use dashmap::DashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;

/// Broadcast channel capacity for progress events. Sized so a momentary stall
/// in the frontend (hundreds of ms) doesn't drop lifecycle events
/// (file_download_complete, job_end) for a slow consumer — `broadcast` silently
/// drops the oldest message for lagging receivers once capacity is reached.
pub const WS_CHANNEL_CAPACITY: usize = 2048;

pub struct TaskEntry {
    pub status: std::sync::Mutex<TaskStatus>,
    pub tx: broadcast::Sender<String>,
    pub cancel: Arc<AtomicBool>,
}

impl TaskEntry {
    pub fn new(task_id: &str) -> Self {
        let (tx, _) = broadcast::channel(WS_CHANNEL_CAPACITY);
        Self {
            status: std::sync::Mutex::new(TaskStatus {
                task_id: task_id.to_string(),
                status: "pending".into(),
                error: None,
            }),
            tx,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn set_running(&self) {
        self.status.lock().unwrap().status = "running".into();
    }

    pub fn set_completed(&self) {
        self.status.lock().unwrap().status = "completed".into();
    }

    pub fn set_failed(&self, err: &str) {
        let mut s = self.status.lock().unwrap();
        s.status = "failed".into();
        s.error = Some(err.to_string());
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

pub struct AppState {
    pub tasks: DashMap<String, Arc<TaskEntry>>,
    pub http: reqwest::Client,
}

impl AppState {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            tasks: DashMap::new(),
            http,
        }
    }

    /// Register a new task and return its entry.
    pub fn register_task(&self, task_id: &str) -> Arc<TaskEntry> {
        let entry = Arc::new(TaskEntry::new(task_id));
        self.tasks.insert(task_id.to_string(), entry.clone());
        entry
    }

    pub fn get_task(&self, task_id: &str) -> Option<Arc<TaskEntry>> {
        self.tasks.get(task_id).map(|r| r.value().clone())
    }

    /// Subscribe to a task's broadcast channel.
    pub fn subscribe(&self, task_id: &str) -> Option<broadcast::Receiver<String>> {
        self.tasks.get(task_id).map(|e| e.tx.subscribe())
    }


}
