use serde::{Deserialize, Serialize};

// ── HTTP request bodies ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct InstallRequest {
    pub gamedir: String,
    pub tempdir: Option<String>,
    pub install_reltype: String, // "os" | "cn" | "bb"
    pub game_type: String,       // "hk4e" | "nap"
}

#[derive(Debug, Clone, Deserialize)]
pub struct RepairRequest {
    pub gamedir: String,
    pub tempdir: Option<String>,
    pub game_type: String,
    pub repair_mode: String, // "quick" | "reliable"
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateRequest {
    pub gamedir: String,
    pub tempdir: Option<String>,
    pub game_type: String,
    pub predownload: bool,
}

// ── HTTP response types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct TaskResponse {
    pub task_id: String,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStatus {
    pub task_id: String,
    pub status: String, // "pending" | "running" | "completed" | "failed"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OnlineGameInfo {
    pub game_type: String,
    pub version: String,
    pub install_size: i64,
    pub updatable_versions: Vec<String>,
    pub release_type: String,
    pub pre_download: bool,
    pub pre_download_version: String,
    pub error: Option<String>,
}

// ── Internal operation parameters ───────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct InstallParams {
    pub gamedir: std::path::PathBuf,
    pub tempdir: std::path::PathBuf,
    pub install_reltype: String,
    pub game_type: String,
}

#[derive(Debug, Clone)]
pub struct RepairParams {
    pub gamedir: std::path::PathBuf,
    pub tempdir: std::path::PathBuf,
    pub game_type: String,
    pub repair_mode: String,
}

#[derive(Debug, Clone)]
pub struct UpdateParams {
    pub gamedir: std::path::PathBuf,
    pub tempdir: std::path::PathBuf,
    pub game_type: String,
    pub predownload: bool,
}
