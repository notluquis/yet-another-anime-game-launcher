/// Fresh-install operation: downloads all chunks and assembles game files.
use anyhow::{Context, Result};
use futures_util::{stream, StreamExt, TryStreamExt};
use serde_json::json;
use std::collections::HashSet;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tokio::io::AsyncWriteExt;

use crate::downloader::{verify_md5, write_chunk};
use crate::hoyo::{get_branch, get_build, ids_for, select_category};
use crate::manifest::{download_manifest, path_safety_check, update_config_version, Manifest};
use crate::models::InstallParams;
use crate::state::TaskEntry;

/// Number of files downloaded in parallel.
const FILE_CONCURRENCY: usize = 4;

/// Game-specific `config.ini` templates written before downloading begins.
fn config_ini_template(game_type: &str, rel_type: &str) -> Option<&'static str> {
    match (game_type, rel_type) {
        ("hk4e", "os") => Some("[General]\r\nchannel=1\r\ncps=mihoyo\r\ngame_version=0.0.0\r\nsdk_version=\r\nsub_channel=0\r\n"),
        ("hk4e", "cn") => Some("[General]\r\nchannel=1\r\ncps=mihoyo\r\ngame_version=0.0.0\r\nsdk_version=\r\nsub_channel=1\r\n"),
        ("hk4e", "bb") => Some("[General]\r\nchannel=14\r\ncps=bilibili\r\ngame_version=0.0.0\r\nsdk_version=\r\nsub_channel=0\r\n"),
        ("nap",  "os") => Some("[General]\r\nchannel=1\r\ncps=mihoyo\r\ngame_version=0.0.0\r\nsdk_version=\r\nsub_channel=0\r\n"),
        ("nap",  "cn") => Some("[General]\r\nchannel=1\r\ncps=mihoyo\r\ngame_version=0.0.0\r\nsdk_version=\r\nsub_channel=1\r\n"),
        _ => None,
    }
}

pub async fn perform_install(
    http: reqwest::Client,
    params: InstallParams,
    entry: Arc<TaskEntry>,
) -> Result<()> {
    let gamedir = params.gamedir;
    let tempdir = params.tempdir;

    tokio::fs::create_dir_all(&gamedir).await?;
    tokio::fs::create_dir_all(&tempdir).await?;

    // Write config.ini skeleton
    if let Some(tmpl) = config_ini_template(&params.game_type, &params.install_reltype) {
        tokio::fs::write(gamedir.join("config.ini"), tmpl).await?;
    }

    // ── Fetch HoYo API ──────────────────────────────────────────────────────
    let ids = ids_for(&params.install_reltype, &params.game_type)?;
    let branch = get_branch(&http, &ids, false).await?;
    let build_data = get_build(&http, &ids, &branch).await?;

    let tag = build_data["tag"]
        .as_str()
        .unwrap_or("0.0.0")
        .to_string();

    // ── Download manifest ───────────────────────────────────────────────────
    let category = select_category(&build_data, "game")?;
    let manifest: Manifest = download_manifest(&http, category).await?;

    let chunk_url_prefix = category["chunk_download"]["url_prefix"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("chunk_download.url_prefix missing"))?
        .to_string();

    // Update config.ini with the real version
    let config_path = gamedir.join("config.ini");
    if config_path.exists() {
        let content = tokio::fs::read_to_string(&config_path).await?;
        let updated = update_config_version(&content, &tag);
        tokio::fs::write(&config_path, &updated).await?;
    }

    // ── Compute total download size ─────────────────────────────────────────
    let total_size: u64 = manifest.files.iter()
        .flat_map(|f| f.chunks.iter())
        .map(|c| c.compressed_size as u64)
        .sum();
    let file_count = manifest.files.len();

    send(&entry, "download_summary", json!({
        "game_version": tag,
        "download_size": total_size,
        "download_file_count": file_count,
        "download_categories": ["game"],
    }));

    // ── Download files (FILE_CONCURRENCY files in parallel) ─────────────────
    // Sort: priority files first (globalgamemanagers/pkg_version), then root, then subdirs.
    let mut files = manifest.files.clone();
    files.sort_by_key(|f| {
        let is_root = !f.filename.contains('/');
        let is_priority = f.filename.contains("globalgamemanagers") || f.filename.contains("pkg_version");
        if is_priority { 0i32 } else if is_root { 1 } else { 2 }
    });

    // Shared atomic counter for total downloaded bytes and session-level speed.
    let downloaded_bytes = Arc::new(AtomicU64::new(0));
    let session_start = std::time::Instant::now();

    // Shared chunk_url_prefix (avoid cloning the string per-task).
    let chunk_url_prefix = Arc::<str>::from(chunk_url_prefix.as_str());

    let dl_result = stream::iter(files.iter().cloned())
        .map(Ok::<_, anyhow::Error>)
        .try_for_each_concurrent(FILE_CONCURRENCY, |file_info| {
            let http = http.clone();
            let entry = entry.clone();
            let db = Arc::clone(&downloaded_bytes);
            let prefix = Arc::clone(&chunk_url_prefix);
            let tempdir = tempdir.clone();
            let gamedir = gamedir.clone();

            async move {
                if entry.is_cancelled() {
                    return Err(anyhow::anyhow!("cancelled"));
                }

                // flags == 64 → directory entry
                if file_info.flags == 64 {
                    send(&entry, "file_download_skipped", json!({
                        "filename": file_info.filename,
                        "reason": "directory",
                    }));
                    return Ok(());
                }

                path_safety_check(&file_info.filename)?;
                let dest = gamedir.join(&file_info.filename);

                // Skip if already complete in game dir
                if dest.exists() {
                    if let Ok(meta) = tokio::fs::metadata(&dest).await {
                        if meta.len() == file_info.size as u64 {
                            send(&entry, "file_download_skipped", json!({
                                "filename": file_info.filename,
                                "reason": "exists",
                            }));
                            return Ok(());
                        }
                    }
                }

                send(&entry, "file_download_start", json!({ "filename": file_info.filename }));

                // Use full relative path under tempdir/partial/ to avoid basename
                // collisions when different files have the same filename component.
                let tmp_assembled = tempdir.join("partial").join(&file_info.filename);
                if let Some(p) = tmp_assembled.parent() {
                    tokio::fs::create_dir_all(p).await?;
                }

                // Sidecar file tracking which chunk indices have been written.
                // Enables chunk-level resume: if interrupted, only missing chunks
                // are re-downloaded on the next run.
                let chunks_done_path = tmp_assembled.with_file_name(format!(
                    "{}.chunks",
                    tmp_assembled.file_name().unwrap_or_default().to_string_lossy()
                ));

                // Load resume state: valid only when both the assembled file AND
                // the sidecar exist. Otherwise start fresh (truncate + pre-alloc).
                let done_set: HashSet<usize> =
                    if tmp_assembled.exists() && chunks_done_path.exists() {
                        tokio::fs::read_to_string(&chunks_done_path).await
                            .unwrap_or_default()
                            .lines()
                            .filter_map(|l| l.trim().parse().ok())
                            .collect()
                    } else {
                        let fh = std::fs::File::create(&tmp_assembled)
                            .with_context(|| format!("create {}", tmp_assembled.display()))?;
                        fh.set_len(file_info.size as u64)?;
                        tokio::fs::remove_file(&chunks_done_path).await.ok();
                        HashSet::new()
                    };

                // Pre-credit already-downloaded chunks to the global counter so
                // the overall progress bar starts at the correct position.
                let pre_bytes: u64 = file_info.chunks.iter().enumerate()
                    .filter(|(i, _)| done_set.contains(i))
                    .map(|(_, c)| c.compressed_size as u64)
                    .sum();
                if pre_bytes > 0 {
                    db.fetch_add(pre_bytes, Ordering::Relaxed);
                    send(&entry, "file_download_resumed", json!({
                        "filename": file_info.filename,
                        "chunks_done": done_set.len(),
                        "chunks_total": file_info.chunks.len(),
                        "pre_downloaded_bytes": pre_bytes,
                    }));
                }

                for (i, chunk) in file_info.chunks.iter().enumerate() {
                    if entry.is_cancelled() {
                        return Err(anyhow::anyhow!("cancelled"));
                    }

                    // Skip chunks already written in a previous session.
                    if done_set.contains(&i) {
                        continue;
                    }

                    let result = write_chunk(
                        &http,
                        &prefix,
                        &chunk.chunk_id,
                        chunk.compressed_size,
                        chunk.offset,
                        &tmp_assembled,
                    )
                    .await
                    .with_context(|| format!("chunk {} of {}", i + 1, file_info.filename))?;

                    // Persist chunk index to the sidecar before updating the counter
                    // so that a crash between the two leaves the sidecar correct.
                    {
                        let mut f = tokio::fs::OpenOptions::new()
                            .create(true).append(true)
                            .open(&chunks_done_path).await?;
                        f.write_all(format!("{i}\n").as_bytes()).await?;
                    }

                    // Atomic add; compute running-average speed = total / elapsed.
                    let cur = db.fetch_add(chunk.compressed_size as u64, Ordering::Relaxed)
                        + chunk.compressed_size as u64;
                    let elapsed = session_start.elapsed().as_secs_f64().max(0.001);
                    let download_speed = cur as f64 / elapsed;
                    // write_ms > 0 guard avoids division by zero on fast writes
                    let write_speed = if result.write_ms > 0 {
                        result.written as f64 / (result.write_ms as f64 / 1000.0)
                    } else {
                        result.written as f64 / 0.001
                    };

                    let percent = if file_info.size > 0 {
                        (chunk.offset + result.written as u64) as f64 / file_info.size as f64 * 100.0
                    } else {
                        100.0
                    };

                    send(&entry, "chunk_progress", json!({
                        "filename": file_info.filename,
                        "total_chunks": file_info.chunks.len(),
                        "current_chunk": i + 1,
                        "progress_percent": percent,
                        "current_byte": chunk.offset + result.written as u64,
                        "total_bytes": file_info.size,
                        "chunk_size": chunk.compressed_size,
                        "download_ms": result.download_ms,
                        "write_ms": result.write_ms,
                        "overall_progress": {
                            "downloaded_size": cur,
                            "total_size": total_size,
                            "overall_percent": cur as f64 / total_size.max(1) as f64 * 100.0,
                            "download_speed": download_speed as u64,
                            "write_speed": write_speed as u64,
                        },
                    }));
                }

                // Verify MD5 — catches both download corruption and stale resumed files.
                if !verify_md5(&tmp_assembled, &file_info.md5).await? {
                    tokio::fs::remove_file(&tmp_assembled).await.ok();
                    tokio::fs::remove_file(&chunks_done_path).await.ok();
                    anyhow::bail!("MD5 mismatch for file: {}", file_info.filename);
                }

                // Sidecar no longer needed once the file is verified.
                tokio::fs::remove_file(&chunks_done_path).await.ok();

                // Move to game directory
                if let Some(parent) = dest.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                if tokio::fs::rename(&tmp_assembled, &dest).await.is_err() {
                    tokio::fs::copy(&tmp_assembled, &dest).await
                        .with_context(|| format!("copy {} to game dir", file_info.filename))?;
                    tokio::fs::remove_file(&tmp_assembled).await.ok();
                }

                send(&entry, "file_download_complete", json!({
                    "filename": file_info.filename,
                    "file_size": file_info.size,
                }));

                Ok(())
            }
        })
        .await;

    match dl_result {
        Ok(()) => {}
        Err(e) if e.to_string() == "cancelled" => {
            send_error(&entry, "cancelled");
            return Ok(());
        }
        Err(e) => return Err(e),
    }

    // Update config.ini to final version
    if config_path.exists() {
        let content = tokio::fs::read_to_string(&config_path).await?;
        let updated = update_config_version(&content, &tag);
        tokio::fs::write(&config_path, updated).await?;
    }

    Ok(())
}

fn send(entry: &Arc<TaskEntry>, kind: &str, mut payload: serde_json::Value) {
    payload["type"] = serde_json::Value::String(kind.to_string());
    payload["task_id"] = serde_json::Value::String(
        entry.status.lock().unwrap().task_id.clone(),
    );
    let s = serde_json::to_string(&payload).unwrap_or_default();
    let _ = entry.tx.send(s);
}

fn send_error(entry: &Arc<TaskEntry>, error: &str) {
    send(entry, "job_error", json!({ "error": error }));
}
