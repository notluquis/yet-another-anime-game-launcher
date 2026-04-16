mod downloader;
mod hpatchz;
mod hoyo;
mod install;
mod manifest;
mod models;
mod repair;
mod state;
mod update;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use models::{InstallRequest, InstallParams, OnlineGameInfo, RepairRequest, RepairParams,
             TaskResponse, TaskStatus, UpdateRequest, UpdateParams};
use serde::Deserialize;
use serde_json::json;
use state::AppState;
use std::sync::Arc;
use tower_http::cors::CorsLayer;

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sophon_server=info,warn".into()),
        )
        .init();

    let port: u16 = std::env::var("SOPHON_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8000);
    let host = std::env::var("SOPHON_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());

    // Watch parent PIDs and exit when they die.
    //
    // We watch two PIDs:
    //  1. TERMINATE_WITH_PID (if set by the JS launcher) — intended to be the
    //     Neutralino/launcher process PID.
    //  2. The *actual* parent PID at startup (getppid) — reliable fallback in
    //     case the JS passes an incorrect or recycled PID.  When Neutralino
    //     exits, our immediate parent (the shell or native process that spawned
    //     us) also exits, and sophon-server is reparented to launchd (PID 1).
    //     Watching getppid() and exiting when it changes to 1 catches this.
    if let Ok(pid_str) = std::env::var("TERMINATE_WITH_PID") {
        if let Ok(pid) = pid_str.parse::<u32>() {
            tokio::spawn(watch_pid(pid));
        }
    }
    // Always watch the actual parent PID as a reliable safety net.
    #[cfg(unix)]
    {
        let ppid = unsafe { libc::getppid() } as u32;
        tokio::spawn(watch_pid(ppid));
    }

    let state = Arc::new(AppState::new());

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/install", post(handle_install))
        .route("/api/repair",  post(handle_repair))
        .route("/api/update",  post(handle_update))
        .route("/api/tasks/{task_id}/status", get(get_task_status))
        .route("/api/tasks/{task_id}", delete(cancel_task))
        .route("/api/game/online_info", get(get_online_info))
        .route("/ws/{task_id}", get(ws_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("{host}:{port}");
    tracing::info!("Sophon server listening on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind");
    axum::serve(listener, app).await.expect("Server error");
}

// ── Health ───────────────────────────────────────────────────────────────────

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "healthy" }))
}

// ── Task handlers ─────────────────────────────────────────────────────────────

async fn handle_install(
    State(state): State<Arc<AppState>>,
    Json(req): Json<InstallRequest>,
) -> Json<TaskResponse> {
    let task_id = uuid::Uuid::new_v4().to_string();
    let entry = state.register_task(&task_id);

    let gamedir = std::path::PathBuf::from(&req.gamedir);
    let tempdir = req
        .tempdir
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| gamedir.join(".tmp"));

    let params = InstallParams {
        gamedir,
        tempdir,
        install_reltype: req.install_reltype,
        game_type: req.game_type,
    };

    let http = state.http.clone();
    let entry2 = entry.clone();
    tokio::spawn(async move {
        entry2.set_running();
        send_event(&entry2, "job_start", json!({}));
        match install::perform_install(http, params, entry2.clone()).await {
            Ok(_) => {
                entry2.set_completed();
                send_event(&entry2, "job_end", json!({}));
            }
            Err(e) => {
                let msg = format!("{e:#}");
                tracing::error!("install failed: {msg}");
                entry2.set_failed(&msg);
                send_event(&entry2, "error", json!({ "error": msg }));
            }
        }
    });

    Json(TaskResponse {
        task_id,
        status: "pending".into(),
        message: "Task started".into(),
    })
}

async fn handle_repair(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RepairRequest>,
) -> Json<TaskResponse> {
    let task_id = uuid::Uuid::new_v4().to_string();
    let entry = state.register_task(&task_id);

    let gamedir = std::path::PathBuf::from(&req.gamedir);
    let tempdir = req
        .tempdir
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| gamedir.join(".tmp"));

    let params = RepairParams {
        gamedir,
        tempdir,
        game_type: req.game_type,
        repair_mode: req.repair_mode,
    };

    let http = state.http.clone();
    let entry2 = entry.clone();
    tokio::spawn(async move {
        entry2.set_running();
        send_event(&entry2, "job_start", json!({}));
        match repair::perform_repair(http, params, entry2.clone()).await {
            Ok(_) => {
                entry2.set_completed();
                send_event(&entry2, "job_end", json!({}));
            }
            Err(e) => {
                let msg = format!("{e:#}");
                tracing::error!("repair failed: {msg}");
                entry2.set_failed(&msg);
                send_event(&entry2, "error", json!({ "error": msg }));
            }
        }
    });

    Json(TaskResponse {
        task_id,
        status: "pending".into(),
        message: "Task started".into(),
    })
}

async fn handle_update(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateRequest>,
) -> Json<TaskResponse> {
    let task_id = uuid::Uuid::new_v4().to_string();
    let entry = state.register_task(&task_id);

    let gamedir = std::path::PathBuf::from(&req.gamedir);
    let tempdir = req
        .tempdir
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| gamedir.join(".tmp"));

    let params = UpdateParams {
        gamedir,
        tempdir,
        game_type: req.game_type,
        predownload: req.predownload,
    };

    let http = state.http.clone();
    let entry2 = entry.clone();
    tokio::spawn(async move {
        entry2.set_running();
        send_event(&entry2, "job_start", json!({}));
        match update::perform_update(http, params, entry2.clone()).await {
            Ok(_) => {
                entry2.set_completed();
                send_event(&entry2, "job_end", json!({}));
            }
            Err(e) => {
                let msg = format!("{e:#}");
                tracing::error!("update failed: {msg}");
                entry2.set_failed(&msg);
                send_event(&entry2, "error", json!({ "error": msg }));
            }
        }
    });

    Json(TaskResponse {
        task_id,
        status: "pending".into(),
        message: "Task started".into(),
    })
}

async fn get_task_status(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    match state.get_task(&task_id) {
        Some(entry) => {
            let s = entry.status.lock().unwrap().clone();
            Json(s).into_response()
        }
        None => Json(TaskStatus {
            task_id: task_id.clone(),
            status: String::new(),
            error: Some("Task not found".into()),
        })
        .into_response(),
    }
}

async fn cancel_task(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<String>,
) -> Json<serde_json::Value> {
    if let Some(entry) = state.get_task(&task_id) {
        entry
            .cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
    Json(json!({ "message": format!("Task {task_id} cancelled") }))
}

// ── Online game info ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct OnlineInfoQuery {
    game: String,
    reltype: String,
}

async fn get_online_info(
    State(state): State<Arc<AppState>>,
    Query(q): Query<OnlineInfoQuery>,
) -> Json<OnlineGameInfo> {
    match fetch_online_info(&state.http, &q.game, &q.reltype).await {
        Ok(info) => Json(info),
        Err(e) => Json(OnlineGameInfo {
            game_type: String::new(),
            version: String::new(),
            install_size: 0,
            updatable_versions: vec![],
            release_type: q.reltype.clone(),
            pre_download: false,
            pre_download_version: "0.0.0".into(),
            error: Some(e.to_string()),
        }),
    }
}

async fn fetch_online_info(
    http: &reqwest::Client,
    game: &str,
    reltype: &str,
) -> anyhow::Result<OnlineGameInfo> {
    use hoyo::{get_branch, get_build, ids_for, select_category};
    use manifest::download_manifest;

    let ids = ids_for(reltype, game)?;

    // Main branch: version + install_size + diff_tags
    let main_branch = get_branch(http, &ids, false).await?;
    let build_data = get_build(http, &ids, &main_branch).await?;
    let category = select_category(&build_data, "game")?;
    let manifest: manifest::manifest_proto::Manifest = download_manifest(http, category).await?;

    let install_size: i64 = manifest
        .files
        .iter()
        .flat_map(|f| f.chunks.iter())
        .map(|c| c.compressed_size as i64)
        .sum();

    // Pre-download branch (may not exist)
    let (pre_download, pre_download_version) =
        match get_branch(http, &ids, true).await {
            Ok(b) => (true, b.tag),
            Err(_) => (false, "0.0.0".to_string()),
        };

    Ok(OnlineGameInfo {
        game_type: game.to_string(),
        version: main_branch.tag.clone(),
        install_size,
        updatable_versions: main_branch.diff_tags,
        release_type: reltype.to_string(),
        pre_download,
        pre_download_version,
        error: None,
    })
}

// ── WebSocket ─────────────────────────────────────────────────────────────────

async fn ws_handler(
    ws: WebSocketUpgrade,
    Path(task_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, task_id, state))
}

async fn handle_socket(mut socket: WebSocket, task_id: String, state: Arc<AppState>) {
    let mut rx = match state.subscribe(&task_id) {
        Some(r) => r,
        None => {
            let _ = socket
                .send(Message::Text(
                    json!({ "type": "error", "error": "task not found" })
                        .to_string()
                        .into(),
                ))
                .await;
            return;
        }
    };

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(msg) => {
                        if socket.send(Message::Text(msg.into())).await.is_err() {
                            break;
                        }
                        // If this was a terminal message, close gracefully
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // Slow consumer: continue draining
                    }
                }
            }
            // Keep the WS alive; close when the client disconnects
            Some(client_msg) = socket.recv() => {
                match client_msg {
                    Ok(Message::Close(_)) | Err(_) => break,
                    _ => {}
                }
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn send_event(entry: &Arc<state::TaskEntry>, kind: &str, mut payload: serde_json::Value) {
    let task_id = entry.status.lock().unwrap().task_id.clone();
    payload["type"] = serde_json::Value::String(kind.to_string());
    payload["task_id"] = serde_json::Value::String(task_id);
    let s = serde_json::to_string(&payload).unwrap_or_default();
    let _ = entry.tx.send(s);
}

/// Watch `pid` and terminate this process when it disappears.
///
/// On macOS we use kqueue(2) + EVFILT_PROC/NOTE_EXIT, which watches the
/// actual process object rather than just the numeric PID.  This is immune
/// to PID reuse (the #1 failure mode of `kill -0` polling: the old PID gets
/// recycled by a new process before the next poll, making the server believe
/// the parent is still alive indefinitely).
///
/// On other platforms we fall back to 200 ms `kill -0` polling.
async fn watch_pid(pid: u32) {
    #[cfg(target_os = "macos")]
    {
        // Spawn a dedicated blocking thread for the kqueue wait so we do not
        // block a Tokio worker thread.
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        std::thread::spawn(move || {
            if watch_pid_kqueue(pid) {
                let _ = tx.send(());
            }
        });
        // If kqueue setup fails the thread returns false and we never receive,
        // so fall through to the polling fallback below.
        if rx.await.is_ok() {
            tracing::info!("Parent PID {pid} gone (kqueue) — shutting down");
            std::process::exit(0);
        }
        // kqueue failed — fall through to polling
    }

    // Polling fallback (Linux / kqueue setup failure)
    let pid_str = pid.to_string();
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let alive = std::process::Command::new("kill")
            .args(["-0", &pid_str])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !alive {
            tracing::info!("Parent PID {pid} gone (poll) — shutting down");
            std::process::exit(0);
        }
    }
}

/// macOS kqueue-based process-exit watcher.
/// Returns `true` when the process exits, `false` if setup failed.
#[cfg(target_os = "macos")]
fn watch_pid_kqueue(pid: u32) -> bool {
    use libc::{kevent, kqueue, EVFILT_PROC, EV_ADD, EV_ENABLE, NOTE_EXIT};
    unsafe {
        let kq = kqueue();
        if kq < 0 {
            return false;
        }

        let change = kevent {
            ident: pid as libc::uintptr_t,
            filter: EVFILT_PROC as i16,
            flags: (EV_ADD | EV_ENABLE) as u16,
            fflags: NOTE_EXIT,
            data: 0,
            udata: std::ptr::null_mut(),
        };

        // Register interest in the process-exit event.
        let ret = kevent(kq, &change, 1, std::ptr::null_mut(), 0, std::ptr::null());
        if ret < 0 {
            libc::close(kq);
            return false;
        }

        // Block until the event fires (no timeout — we want to wait forever).
        let mut event: kevent = std::mem::zeroed();
        loop {
            let n = kevent(kq, std::ptr::null(), 0, &mut event, 1, std::ptr::null());
            if n > 0 {
                libc::close(kq);
                return true;
            }
            // n == 0 → spurious wakeup, retry; n < 0 → error, bail
            if n < 0 {
                libc::close(kq);
                return false;
            }
        }
    }
}
